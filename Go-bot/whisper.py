import os
import sys
import re
import random
from groq import Groq

def get_groq_api_keys():
    base_dir = os.path.dirname(os.path.abspath(__file__))
    api_keys_path = os.path.join(base_dir, "..", "src", "api_keys.rs")
    keys = []
    try:
        with open(api_keys_path, "r") as f:
            content = f.read()
            pattern = r'pub\s+const\s+GROQ_KEY.*:\s*&str\s*=\s*"([^"]+)"'
            keys = re.findall(pattern, content)
    except Exception as e:
        print(f"Error reading api_keys.rs: {e}", file=sys.stderr)
    return keys

groq_keys = get_groq_api_keys()
if not groq_keys:
    api_key = os.environ.get("GROQ_API_KEY")
    if not api_key:
        print("Error: No GROQ API key found", file=sys.stderr)
        sys.exit(1)
else:
    api_key = random.choice(groq_keys)

client = Groq(api_key=api_key)

if len(sys.argv) < 2:
    print("Error: Missing audio file path")
    sys.exit(1)

filename = sys.argv[1]

with open(filename, "rb") as file:
    transcription = client.audio.transcriptions.create(
      file=(filename, file.read()),
      model="whisper-large-v3",
      temperature=0,
      response_format="verbose_json",
    )
    print(transcription.text)