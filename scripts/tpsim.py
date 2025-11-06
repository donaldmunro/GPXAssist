#!/usr/bin/env python3
import json
import sys
import time
import os
import tempfile

def main():
    if len(sys.argv) != 5:
        print("Usage: python script.py path increment total sleep")
        sys.exit(1)

    path = sys.argv[1]
    increment = int(sys.argv[2])
    total = int(sys.argv[3])
    sleep_time = float(sys.argv[4])

    while True:
        # Read the file as binary to handle the BOM
        with open(path, 'rb') as f:
            content = f.read()

        # Remove the BOM (0xEF, 0xBB, 0xBF) if present at the start
        if content.startswith(b'\xEF\xBB\xBF'):
            content = content[3:]

        # Decode the content to string
        content_str = content.decode('utf-8')

        # Remove the surrounding array brackets
        # Find first [ and last ]
        start_idx = content_str.find('[')
        end_idx = content_str.rfind(']')

        if start_idx == -1 or end_idx == -1:
            raise ValueError("Invalid JSON format: missing array brackets")

        # Extract the inner JSON content
        json_content = content_str[start_idx+1:end_idx].strip()

        # Handle case where array contains multiple objects
        # For this specific case we expect just one object in the array
        # Parse the JSON content
        data = json.loads(f"[{json_content}]")

        # Update the distance field
        for item in data:
            if "distance" in item:
                item["distance"] += increment

        # Check if any distance exceeds the total
        should_exit = False
        for item in data:
            if "distance" in item and item["distance"] > total:
                should_exit = True
                break

        # Write to a temporary file in the same directory
        dir_path = os.path.dirname(path)
        if not dir_path:
            dir_path = '.'

        with tempfile.NamedTemporaryFile(mode='w', dir=dir_path, delete=False, encoding='utf-8') as tmp_file:
            # Write the BOM and the array structure back
            tmp_file.write('\ufeff')  # Write the BOM (U+FEFF)
            json.dump(data, tmp_file)

        # Atomically move the temporary file to the original path
        os.replace(tmp_file.name, path)

        if should_exit:
            break

        # Sleep for the specified time
        time.sleep(sleep_time)

if __name__ == "__main__":
    main()
