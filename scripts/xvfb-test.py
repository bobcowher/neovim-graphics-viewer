import tkinter as tk, subprocess, time, os, sys, json

root = tk.Tk()
root.geometry("1600x900+0+0")
root.update()

win_id = int(root.wm_frame(), 16)
print(f"window id: {win_id}", flush=True)
os.environ["WINDOWID"] = str(win_id)

img = sys.argv[1] if len(sys.argv) > 1 else "53dd0a9a-5dad-4f78-a794-b630c22750b3.png"

show = json.dumps({"cmd": "show", "path": img, "x": 0, "y": 0, "w": 80, "h": 40, "cols": 200, "rows": 50}) + "\n"
quit_ = json.dumps({"cmd": "quit"}) + "\n"

proc = subprocess.Popen(
    ["./bin/nvim-gfx"],
    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE
)
proc.stdin.write(show.encode())
proc.stdin.flush()
time.sleep(0.8)
proc.stdin.write(quit_.encode())
proc.stdin.flush()

try:
    out, err = proc.communicate(timeout=5)
    print(f"stdout: {out.decode()}", flush=True)
    if err:
        print(f"stderr: {err.decode()}", flush=True)
    print(f"exit: {proc.returncode}", flush=True)
except subprocess.TimeoutExpired:
    proc.kill()
    print("TIMEOUT", flush=True)

root.destroy()
