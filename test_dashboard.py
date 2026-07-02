import requests
import time
import subprocess

# Start relay
relay = subprocess.Popen(["./target/debug/dos-relay"])
time.sleep(1)
# Start dashboard
dash = subprocess.Popen(["./target/debug/dos", "dashboard"])
time.sleep(2)

try:
    r = requests.get("http://127.0.0.1:8080/api/devices")
    print("Status Code:", r.status_code)
    print("Response JSON:", r.json())
finally:
    relay.terminate()
    dash.terminate()
