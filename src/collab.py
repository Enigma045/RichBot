# ============================================================
# CELL 2 — Full Server (TTS + File Search)
# ============================================================
import sys
sys.path.insert(0, "/content/drive/MyDrive/colab_packages")

import json, io, os, subprocess, tempfile, threading
from pathlib import Path
from flask import Flask, request, send_file, jsonify
from pyngrok import ngrok
from neutts import NeuTTS
import soundfile as sf
from sentence_transformers import SentenceTransformer, util

# ── Config ───────────────────────────────────────────────────
PATHS_FILE = "/content/drive/MyDrive/paths.json"
REF_AUDIO  = "/content/drive/MyDrive/dave.wav"
REF_TEXT   = "/content/drive/MyDrive/dave.txt"
TOP_K      = 5

# ── Load TTS model ───────────────────────────────────────────
print("⏳ Loading TTS model...")
tts = NeuTTS(
    backbone_repo="neuphonic/neutts-air-q4-gguf",
    backbone_device="cpu",
    codec_repo="neuphonic/neucodec",
    codec_device="cpu"
)
ref_text  = open(REF_TEXT).read().strip()
ref_codes = tts.encode_reference(REF_AUDIO)
print("✅ TTS model loaded.")

# ── Load search model ────────────────────────────────────────
print("⏳ Loading search model...")
search_model = SentenceTransformer("all-MiniLM-L6-v2")
with open(PATHS_FILE, "r", encoding="utf-8") as f:
    file_paths = json.load(f)
print(f"✅ Search model loaded. {len(file_paths)} paths indexed.")

# ── Helper: clean filename for search ────────────────────────
def clean_filename(path: str) -> str:
    name = Path(path).stem
    for char in [".", "-", "_", "[", "]", "(", ")"]:
        name = name.replace(char, " ")
    return name.strip()

# ── Pre-compute all path embeddings ONCE at startup ───────────
# This avoids re-encoding thousands of paths on every search request.
print("⏳ Pre-computing path embeddings (one-time)...")
clean_names = [clean_filename(p) for p in file_paths]
path_embeddings = search_model.encode(clean_names, convert_to_tensor=True, batch_size=256, show_progress_bar=True)
print(f"✅ Embeddings cached for {len(file_paths)} paths.")

# ── Helper: search files (now just a single matrix op) ────────
def search_files(query: str):
    query_embedding = search_model.encode(query, convert_to_tensor=True)
    # util.cos_sim returns a (1 x N) tensor — get the 1D scores
    scores_tensor = util.cos_sim(query_embedding, path_embeddings)[0]
    scores = [(float(scores_tensor[i]), file_paths[i]) for i in range(len(file_paths))]
    scores.sort(reverse=True)
    return scores[:TOP_K]

# ── Flask app ────────────────────────────────────────────────
app = Flask(__name__)

@app.route("/tts", methods=["POST"])
def synthesize():
    data = request.get_json(force=True)
    text = data.get("text", "").strip()

    if not text:
        return {"error": "empty text"}, 400
    if len(text) > 2000:
        return {"error": "text too long — max 2000 chars"}, 400

    wav = tts.infer(text, ref_codes, ref_text)

    with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as tmp:
        sf.write(tmp.name, wav, 24000, format="WAV")
        wav_path = tmp.name

    ogg_path = wav_path.replace(".wav", ".ogg")
    subprocess.run(
        ["ffmpeg", "-y", "-i", wav_path, "-c:a", "libopus", ogg_path],
        check=True, capture_output=True
    )
    os.unlink(wav_path)

    response = send_file(ogg_path, mimetype="audio/ogg", download_name="speech.ogg")
    os.unlink(ogg_path)
    return response

@app.route("/search", methods=["POST"])
def search():
    data = request.get_json(force=True)
    query = data.get("query", "").strip()

    if not query:
        return {"error": "empty query"}, 400

    results = search_files(query)
    return jsonify([
        {"rank": i + 1, "score": round(score, 4), "path": path}
        for i, (score, path) in enumerate(results)
    ])

@app.route("/ping", methods=["GET"])
def ping():
    return {"status": "alive"}, 200

# ── Start ngrok + Flask ──────────────────────────────────────
public_url = ngrok.connect(5000).public_url
print("=" * 60)
print(f"  TTS URL:    {public_url}/tts")
print(f"  Search URL: {public_url}/search")
print(f"  Paste base URL into main.go")
print("=" * 60)

threading.Thread(
    target=lambda: app.run(port=5000, use_reloader=False),
    daemon=True
).start()