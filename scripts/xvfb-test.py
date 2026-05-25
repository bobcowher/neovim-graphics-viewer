import tkinter as tk, subprocess, time, os, sys, json

def run_binary(window_id: int, commands: list, wait: float = 0.5) -> dict:
    """Send a list of JSON commands to the binary, collect stdout, return parsed events."""
    os.environ["WINDOWID"] = str(window_id)
    proc = subprocess.Popen(
        ["./bin/nvim-gfx"],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )
    for cmd in commands:
        proc.stdin.write((json.dumps(cmd) + "\n").encode())
        proc.stdin.flush()
    time.sleep(wait)
    try:
        proc.stdin.write((json.dumps({"cmd": "quit"}) + "\n").encode())
        proc.stdin.flush()
    except BrokenPipeError:
        pass
    try:
        out, err = proc.communicate(timeout=5)
    except subprocess.TimeoutExpired:
        proc.kill()
        return {"error": "TIMEOUT"}
    lines = [l for l in out.decode().splitlines() if l.strip()]
    events = []
    for l in lines:
        try:
            events.append(json.loads(l))
        except json.JSONDecodeError:
            pass
    return {"events": events, "stderr": err.decode(), "exit": proc.returncode}

def main():
    root = tk.Tk()
    root.geometry("1600x900+0+0")
    root.update()
    win_id = int(root.wm_frame(), 16)
    print(f"window id: {win_id}", flush=True)

    img = sys.argv[1] if len(sys.argv) > 1 else "53dd0a9a-5dad-4f78-a794-b630c22750b3.png"

    # Test 1: image opens and emits ready
    print("Test 1: image open", flush=True)
    result = run_binary(win_id, [
        {"cmd": "show", "path": img, "x": 0, "y": 0, "w": 80, "h": 40, "cols": 200, "rows": 50}
    ])
    assert result["exit"] == 0, f"non-zero exit: {result}"
    assert any(e.get("event") == "ready" for e in result["events"]), \
        f"no ready event: {result}"
    print("  PASS", flush=True)

    # Test 2: nonexistent video emits error (not a crash)
    print("Test 2: bad video path emits error", flush=True)
    result = run_binary(win_id, [
        {"cmd": "show", "path": "/nonexistent/clip.mp4", "x": 0, "y": 0,
         "w": 80, "h": 40, "cols": 200, "rows": 50}
    ], wait=0.2)
    assert any(e.get("event") == "error" for e in result["events"]), \
        f"expected error event for bad video path: {result}"
    print("  PASS", flush=True)

    root.destroy()
    print("All tests passed.", flush=True)

if __name__ == "__main__":
    main()
