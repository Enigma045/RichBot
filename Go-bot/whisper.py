import os
import sys
from groq import Groq

os.environ["GROQ_API_KEY"]
client = Groq()

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