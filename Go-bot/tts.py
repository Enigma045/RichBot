import os
import sys
import argparse
from pathlib import Path
from groq import Groq

os.environ["GROQ_API_KEY"]
client = Groq()

parser = argparse.ArgumentParser()
parser.add_argument("--file", help="Input text file path")
parser.add_argument("--output", help="Output audio file path")
args, unknown = parser.parse_known_args()

if args.file and os.path.exists(args.file):
    with open(args.file, "r", encoding="utf-8") as f:
        text = f.read()
else:
    text = sys.argv[1] if sys.argv[1:] else "hello thanks"

speech_file_path = Path(args.output) if args.output else Path(__file__).parent / "speech.wav"

# Truncate text just in case it's too long for TTS
if len(text) > 4000:
    text = text[:4000]

response = client.audio.speech.create(
  model="canopylabs/orpheus-v1-english",
  voice="autumn",
  response_format="wav",
  input=text,
)
response.write_to_file(speech_file_path)