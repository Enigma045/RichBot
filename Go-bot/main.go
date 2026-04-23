package main

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"os/signal"
	"strings"
	"sync"
	"syscall"
	"time"
	"bufio"

	"github.com/mdp/qrterminal/v3"
	_ "github.com/mattn/go-sqlite3"
	waProto "go.mau.fi/whatsmeow/binary/proto"
	"go.mau.fi/whatsmeow"
	"go.mau.fi/whatsmeow/store/sqlstore"
	"go.mau.fi/whatsmeow/types/events"
	waLog "go.mau.fi/whatsmeow/util/log"
	"google.golang.org/protobuf/proto"
)

const (
	maxMessageLength = 4096
	execTimeout      = 1800 * time.Second
	ttsTimeout       = 300 * time.Second
	maxWorkers       = 5
	maxInputLength   = 3000
)

// ─── CHANGE THESE TO MATCH YOUR MACHINE ─────────────────────────────────────
const analyzerExe = `C:\Users\USER\Rust\Code_analyzer\target\debug\Code_analyzer.exe`
const analyzerDir = `C:\Users\USER\Rust\Code_analyzer`
const planFile    = `C:\Users\USER\Rust\Code_analyzer\plans\plan.txt`

// ─── PASTE YOUR NGROK URL HERE EVERY TIME YOU START COLAB ───────────────────
// Example: "https://abcd-12-34-56-78.ngrok-free.app"
const colabBaseURL = "https://uncomforting-olin-unbewitchingly.ngrok-free.dev"

// ─────────────────────────────────────────────────────────────────────────────

type Job struct {
	ctx    context.Context
	client *whatsmeow.Client
	evt    *events.Message
	msg    string
}

var (
	jobQueue = make(chan Job, 50)
	wg       sync.WaitGroup
)

func sanitizeInput(input string) (string, error) {
	input = strings.TrimSpace(input)
	if len(input) == 0 {
		return "", fmt.Errorf("empty input")
	}
	if len(input) > maxInputLength {
		return "", fmt.Errorf("input too long: %d chars (max %d)", len(input), maxInputLength)
	}
	return input, nil
}

func runAnalyzer(ctx context.Context, client *whatsmeow.Client, evt *events.Message, msg string) (string, error) {
	// We use the context from the caller for timeout management
	var stdoutBuf bytes.Buffer
	cmd := exec.CommandContext(ctx, analyzerExe, "--prompt", msg, "--url", colabBaseURL)
	cmd.Dir = analyzerDir

	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return "", fmt.Errorf("failed to create stdout pipe: %w", err)
	}
	cmd.Stderr = os.Stderr // Pipe stderr to Go console for debugging

	if err := cmd.Start(); err != nil {
		return "", fmt.Errorf("failed to start analyzer: %w", err)
	}

	// Read stdout line by line in real-time
	scanner := bufio.NewScanner(stdout)
	for scanner.Scan() {
		line := scanner.Text()
		stdoutBuf.WriteString(line + "\n")
		fmt.Println("[Analyzer Out]:", line)

		// ── Real-time WhatsApp Forwarding ────────────────────────────────────
		if strings.HasPrefix(line, "🚀 [STAGE]") || strings.HasPrefix(line, "📍 [STEP]") || strings.HasPrefix(line, "✅ [COMPLETE]") {
			sendText(ctx, client, evt, line)
		}

		// ── Real-time Plan Delivery ──────────────────────────────────────────
		if strings.Contains(line, "📋 Brain: Plan saved to") {
			// Small delay to ensure disk has finished writing
			time.Sleep(200 * time.Millisecond)
			sendPlanToWhatsApp(ctx, client, evt)
		}
	}

	if err := cmd.Wait(); err != nil {
		// If it's a non-zero exit but we have output, we might still want to return it
		// but usually analyzer error means failure.
		return stdoutBuf.String(), fmt.Errorf("analyzer exited with error: %w", err)
	}

	return strings.TrimSpace(stdoutBuf.String()), nil
}

func runFilter(originalPrompt, aiOutput string) (string, error) {
	ctx, cancel := context.WithTimeout(context.Background(), execTimeout)
	defer cancel()

	var stdoutBuf, stderrBuf bytes.Buffer
	cmd := exec.CommandContext(ctx, analyzerExe, "--filter", originalPrompt, aiOutput, "--url", colabBaseURL)
	cmd.Dir = analyzerDir
	cmd.Stdout = &stdoutBuf
	cmd.Stderr = &stderrBuf

	err := cmd.Run()
	outputStr := strings.TrimSpace(stdoutBuf.String())
	stderrStr := strings.TrimSpace(stderrBuf.String())

	logToInternalAudit(outputStr, stderrStr)

	if ctx.Err() == context.DeadlineExceeded {
		return "", fmt.Errorf("filter timed out after %v", execTimeout)
	}
	if err != nil {
		return "", fmt.Errorf("filter error: %v\nStderr:\n%s", err, stderrStr)
	}

	return outputStr, nil
}

// sendPlanToWhatsApp reads plans/plan.txt from disk and sends it as a
// WhatsApp document.
func sendPlanToWhatsApp(ctx context.Context, client *whatsmeow.Client, evt *events.Message) {
	data, err := os.ReadFile(planFile)
	if err != nil {
		fmt.Printf("⚠️  plan.txt not found (%v), skipping plan delivery\n", err)
		return
	}
	content := string(data)
	if strings.TrimSpace(content) == "" {
		return
	}

	logToAudit(fmt.Sprintf("[Plan sent]:\n%s", content))
	fileData := []byte(content)
	resp, err := uploadWithRetry(ctx, client, fileData, whatsmeow.MediaDocument)
	if err != nil {
		fmt.Printf("❌ Failed to upload plan.txt after retries: %v\n", err)
		return
	}

	fileName := "plan.txt"
	client.SendMessage(ctx, evt.Info.Chat, &waProto.Message{
		DocumentMessage: &waProto.DocumentMessage{
			URL:           &resp.URL,
			DirectPath:    &resp.DirectPath,
			MediaKey:      resp.MediaKey,
			FileEncSHA256: resp.FileEncSHA256,
			FileSHA256:    resp.FileSHA256,
			FileLength:    &resp.FileLength,
			Mimetype:      proto.String("text/plain"),
			FileName:      &fileName,
		},
	})
	fmt.Println("📋 plan.txt sent to WhatsApp")
}

// callColabTTS sends text to the Colab Flask API and returns
// a path to a local temp .ogg file ready for WhatsApp.
func callColabTTS(ctx context.Context, text string) (string, error) {
	payload, err := json.Marshal(map[string]string{"text": text})
	if err != nil {
		return "", fmt.Errorf("TTS payload marshal failed: %w", err)
	}

	url := fmt.Sprintf("%s/tts", strings.TrimSuffix(colabBaseURL, "/"))
	req, err := http.NewRequestWithContext(ctx, "POST", url, bytes.NewReader(payload))
	if err != nil {
		return "", fmt.Errorf("TTS request build failed: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("ngrok-skip-browser-warning", "any")
	req.Header.Set("Content-Type", "application/json")

	client := &http.Client{Timeout: ttsTimeout}
	resp, err := client.Do(req)
	if err != nil {
		return "", fmt.Errorf("TTS request failed (is Colab running?): %w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		body, _ := io.ReadAll(resp.Body)
		return "", fmt.Errorf("TTS API returned %d: %s", resp.StatusCode, string(body))
	}

	// Save response to a temp .ogg file
	tmpFile, err := os.CreateTemp("", "speech_*.ogg")
	if err != nil {
		return "", fmt.Errorf("temp file creation failed: %w", err)
	}
	defer tmpFile.Close()

	if _, err := io.Copy(tmpFile, resp.Body); err != nil {
		os.Remove(tmpFile.Name())
		return "", fmt.Errorf("saving audio failed: %w", err)
	}

	return tmpFile.Name(), nil
}

func logToAudit(text string) {
	f, err := os.OpenFile("audit.txt", os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err != nil {
		fmt.Println("Error opening audit.txt:", err)
		return
	}
	defer f.Close()
	timestamp := time.Now().Format("2006-01-02 15:04:05")
	entry := fmt.Sprintf("[%s] Sent to WhatsApp:\n%s\n\n", timestamp, text)
	if _, err := f.WriteString(entry); err != nil {
		fmt.Println("Error writing to audit.txt:", err)
	}
}

func logToInternalAudit(stdout, stderr string) {
	f, err := os.OpenFile("audit_internal.txt", os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err != nil {
		fmt.Println("Error opening audit_internal.txt:", err)
		return
	}
	defer f.Close()
	timestamp := time.Now().Format("2006-01-02 15:04:05")
	entry := fmt.Sprintf("[%s] INTERNAL LOG:\nSTDOUT:\n%s\nSTDERR:\n%s\n%s\n", 
		timestamp, stdout, stderr, strings.Repeat("-", 40))
	if _, err := f.WriteString(entry); err != nil {
		fmt.Println("Error writing to audit_internal.txt:", err)
	}
}

func sendText(ctx context.Context, client *whatsmeow.Client, evt *events.Message, text string) {
	logToAudit(text)
	client.SendMessage(ctx, evt.Info.Chat, &waProto.Message{
		Conversation: proto.String(text),
	})
}

func uploadWithRetry(ctx context.Context, client *whatsmeow.Client, data []byte, mediaType whatsmeow.MediaType) (whatsmeow.UploadResponse, error) {
	const maxAttempts = 3
	backoff := 2 * time.Second
	var lastErr error
	for attempt := 1; attempt <= maxAttempts; attempt++ {
		resp, err := client.Upload(ctx, data, mediaType)
		if err == nil {
			return resp, nil
		}
		lastErr = err
		fmt.Printf("⚠️  Upload attempt %d/%d failed: %v. Retrying in %v...\n", attempt, maxAttempts, err, backoff)
		time.Sleep(backoff)
		backoff *= 2
	}
	return whatsmeow.UploadResponse{}, fmt.Errorf("upload failed after %d attempts: %w", maxAttempts, lastErr)
}

func sendDocument(ctx context.Context, client *whatsmeow.Client, evt *events.Message, content string) error {
	logToAudit(fmt.Sprintf("[Document Sent (length: %d)]:\n%s", len(content), content))
	fileData := []byte(content)
	resp, err := uploadWithRetry(ctx, client, fileData, whatsmeow.MediaDocument)
	if err != nil {
		return fmt.Errorf("upload failed: %w", err)
	}

	client.SendMessage(ctx, evt.Info.Chat, &waProto.Message{
		DocumentMessage: &waProto.DocumentMessage{
			URL:           &resp.URL,
			DirectPath:    &resp.DirectPath,
			MediaKey:      resp.MediaKey,
			FileEncSHA256: resp.FileEncSHA256,
			FileSHA256:    resp.FileSHA256,
			FileLength:    &resp.FileLength,
			Mimetype:      proto.String("text/plain"),
			FileName:      proto.String("response.txt"),
		},
	})
	return nil
}

func sendAudio(ctx context.Context, client *whatsmeow.Client, evt *events.Message, filePath string) error {
	logToAudit(fmt.Sprintf("[Audio Sent]: %s", filePath))
	data, err := os.ReadFile(filePath)
	if err != nil {
		return err
	}
	resp, err := client.Upload(ctx, data, whatsmeow.MediaAudio)
	if err != nil {
		return err
	}

	_, err = client.SendMessage(ctx, evt.Info.Chat, &waProto.Message{
		AudioMessage: &waProto.AudioMessage{
			URL:           &resp.URL,
			DirectPath:    &resp.DirectPath,
			MediaKey:      resp.MediaKey,
			FileEncSHA256: resp.FileEncSHA256,
			FileSHA256:    resp.FileSHA256,
			FileLength:    &resp.FileLength,
			Mimetype:      proto.String("audio/ogg; codecs=opus"),
			PTT:           proto.Bool(true),
		},
	})
	if err != nil {
		logToAudit(fmt.Sprintf("❌ SendMessage error: %v", err))
		return err
	}
	return nil
}

func processJob(job Job) {
	ctx := job.ctx
	client := job.client
	evt := job.evt

	var clean string
	if audioMsg := evt.Message.GetAudioMessage(); audioMsg != nil {
		sendText(ctx, client, evt, "🎙️ Receiving voice note...")
		data, err := client.Download(ctx, audioMsg)
		if err != nil {
			sendText(ctx, client, evt, fmt.Sprintf("❌ Failed to download audio: %v", err))
			return
		}

		fileName := fmt.Sprintf("audio_%d.ogg", time.Now().UnixNano())
		if err := os.WriteFile(fileName, data, 0600); err != nil {
			sendText(ctx, client, evt, fmt.Sprintf("❌ Failed to save audio: %v", err))
			return
		}
		defer os.Remove(fileName)

		cmd := exec.Command("python", "whisper.py", fileName)
		out, err := cmd.CombinedOutput()
		if err != nil {
			sendText(ctx, client, evt, fmt.Sprintf("❌ Transcription failed: %v\n%s", err, string(out)))
			return
		}
		clean = strings.TrimSpace(string(out))
		if clean == "" {
			sendText(ctx, client, evt, "❌ Transcription was empty")
			return
		}
		sendText(ctx, client, evt, fmt.Sprintf("📝 Transcribed:\n%s", clean))
	} else {
		var err error
		clean, err = sanitizeInput(job.msg)
		if err != nil {
			sendText(ctx, client, evt, fmt.Sprintf("❌ Invalid input: %v", err))
			return
		}
	}

	fmt.Println("📩 Processing prompt:", clean)
	sendText(ctx, client, evt, "⏳ Enigma is processing your request...")

	output, err := runAnalyzer(ctx, client, evt, clean)
	if err != nil {
		sendText(ctx, client, evt, fmt.Sprintf("❌ %v", err))
		return
	}

	// ── Voice note response via Colab TTS ────────────────────
	if strings.Contains(strings.ToLower(clean), "voice note") {
		// New Filtering Step
		filteredOutput, err := runFilter(clean, output)
		if err != nil {
			sendText(ctx, client, evt, fmt.Sprintf("⚠️ Filter failed, sending full response: %v", err))
		} else {
			output = "🌌 Enigma Filtered Response:\n\n" + filteredOutput
		}

		sendText(ctx, client, evt, "🎙️ Generating voice note...")

		ttsCtx, cancel := context.WithTimeout(ctx, ttsTimeout)
		defer cancel()

		// Use filtered output for TTS if it was successful
		ttsInput := output
		if err == nil {
			ttsInput = filteredOutput
		}

		audioPath, err := callColabTTS(ttsCtx, ttsInput)
		if err != nil {
			sendText(ctx, client, evt, fmt.Sprintf("❌ TTS failed: %v", err))
		} else {
			defer os.Remove(audioPath)
			if err := sendAudio(ctx, client, evt, audioPath); err != nil {
				sendText(ctx, client, evt, fmt.Sprintf("❌ Failed to send audio: %v", err))
			}
		}
		// Still send text response below regardless of TTS result
	}

	// ── Text / document response ─────────────────────────────
	if len(output) > maxMessageLength {
		if err := sendDocument(ctx, client, evt, output); err != nil {
			sendText(ctx, client, evt, fmt.Sprintf("❌ Failed to send document: %v", err))
		}
	} else {
		sendText(ctx, client, evt, output)
	}
}

func startWorkers() {
	for i := 0; i < maxWorkers; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for job := range jobQueue {
				processJob(job)
			}
		}()
	}
}

func extractMessage(v *events.Message) string {
	if msg := v.Message.GetConversation(); msg != "" {
		return msg
	}
	if ext := v.Message.GetExtendedTextMessage(); ext != nil {
		return ext.GetText()
	}
	return ""
}

func connectWithRetry(client *whatsmeow.Client) error {
	backoff := time.Second
	for {
		err := client.Connect()
		if err == nil {
			return nil
		}
		fmt.Printf("❌ Connection failed: %v. Retrying in %v...\n", err, backoff)
		time.Sleep(backoff)
		if backoff < 60*time.Second {
			backoff *= 2
		}
	}
}

func main() {
	ctx := context.Background()

	dbLog := waLog.Stdout("DB", "INFO", true)
	container, err := sqlstore.New(ctx, "sqlite3", "file:store.db?_foreign_keys=on", dbLog)
	if err != nil {
		panic(fmt.Sprintf("DB init failed: %v", err))
	}

	deviceStore, err := container.GetFirstDevice(ctx)
	if err != nil {
		panic(fmt.Sprintf("Device store failed: %v", err))
	}

	client := whatsmeow.NewClient(deviceStore, waLog.Stdout("Client", "INFO", true))

	client.AddEventHandler(func(evt interface{}) {
		switch v := evt.(type) {
		case *events.Message:
			if v.Info.IsFromMe {
				return
			}

			msg := extractMessage(v)
			audioMsg := v.Message.GetAudioMessage()
			if msg == "" && audioMsg == nil {
				return
			}

			select {
			case jobQueue <- Job{ctx: ctx, client: client, evt: v, msg: msg}:
			default:
				sendText(ctx, client, v, "⚠️ Bot is busy. Try again shortly.")
			}
		}
	})

	if client.Store.ID == nil {
		qrChan, err := client.GetQRChannel(ctx)
		if err != nil {
			panic(fmt.Sprintf("QR channel failed: %v", err))
		}
		go func() {
			for evt := range qrChan {
				if evt.Event == "code" {
					fmt.Println("📱 Scan QR:")
					qrterminal.GenerateHalfBlock(evt.Code, qrterminal.L, os.Stdout)
				} else {
					fmt.Println("Login event:", evt.Event)
				}
			}
		}()
	}

	startWorkers()

	if err := connectWithRetry(client); err != nil {
		panic(err)
	}

	fmt.Println("🚀 Bot running...")

	go func() {
		for {
			time.Sleep(30 * time.Second)
			if !client.IsConnected() {
				fmt.Println("⚠️ Disconnected. Reconnecting...")
				connectWithRetry(client)
			}
		}
	}()

	c := make(chan os.Signal, 1)
	signal.Notify(c, os.Interrupt, syscall.SIGTERM)
	<-c

	fmt.Println("🛑 Shutting down...")
	close(jobQueue)
	wg.Wait()
	client.Disconnect()
}
