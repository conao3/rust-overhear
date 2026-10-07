import argparse
import json
from pathlib import Path

parser = argparse.ArgumentParser(description="Summarize segment update JSONL from record-segments.mjs")
parser.add_argument("recording", type=Path)
args = parser.parse_args()
by_segment = {}
count = 0
with args.recording.open() as stream:
    for line in stream:
        row = json.loads(line)
        by_segment.setdefault(row["id"], []).append(row)
        count += 1
if not count:
    parser.error("The recording contains no updates")
print(f"Total updates: {count}")
for segment_id, rows in by_segment.items():
    interim = [row for row in rows if row["status"] == "INTERIM"]
    final = [row for row in rows if row["status"] == "FINAL"]
    refined = next((row for row in final if "whisper" in row["asrEngine"]), None)
    translated = next((row for row in rows if row["translations"]), None)
    print(f"id={segment_id} audio={rows[-1]['startMs'] / 1000:.1f}-{rows[-1]['endMs'] / 1000:.1f}s interim={len(interim)}")
    for row in [final[0] if final else None, refined]:
        if row:
            print(f"  t={row['t'] / 1000:.2f} {row['asrEngine']}: {row['sourceText']}")
    if translated:
        translation = translated["translations"][-1]
        print(f"  t={translated['t'] / 1000:.2f} {translation['engineId']}: {translation['text']}")
