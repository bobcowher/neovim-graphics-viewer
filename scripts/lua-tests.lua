-- Headless Lua test harness for nvim-gfx viewer.
-- Stubs jobstart/chansend so we can drive the plugin without spawning the binary
-- or needing a real Kitty-capable terminal.
--
-- Run: ./scripts/lua-tests.sh

local plugin_dir = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":h:h")
vim.opt.rtp:prepend(plugin_dir)

local sent = {}
local jobs_started = {}
local next_job_id = 1000
local notifications = {}

local orig_jobstart  = vim.fn.jobstart
local orig_chansend  = vim.fn.chansend
local orig_jobwait   = vim.fn.jobwait
local orig_jobstop   = vim.fn.jobstop
local orig_resolve   = vim.fn.resolve
local orig_readable  = vim.fn.filereadable
local orig_exec      = vim.fn.executable
local orig_notify    = vim.notify

vim.fn.jobstart  = function(cmd, opts) next_job_id = next_job_id + 1; table.insert(jobs_started, { cmd = cmd, opts = opts, id = next_job_id }); return next_job_id end
vim.fn.chansend  = function(id, data) table.insert(sent, { id = id, data = data }); return #data end
vim.fn.jobwait   = function() return { 0 } end
vim.fn.jobstop   = function() return 1 end
vim.fn.resolve   = function() return "/dev/pts/99" end
vim.fn.filereadable = function() return 1 end
vim.fn.executable   = function() return 1 end
vim.notify       = function(msg, level) table.insert(notifications, { msg = msg, level = level }) end

local viewer = require("nvim-gfx.viewer")

local failures = 0
local function assert_eq(name, got, want)
    if got == want then
        print("  PASS " .. name)
    else
        print("  FAIL " .. name .. " — got " .. tostring(got) .. " want " .. tostring(want))
        failures = failures + 1
    end
end

local function assert_true(name, cond, detail)
    if cond then
        print("  PASS " .. name)
    else
        print("  FAIL " .. name .. (detail and (" — " .. detail) or ""))
        failures = failures + 1
    end
end

local function reset_capture()
    sent = {}
    jobs_started = {}
    notifications = {}
end

-- Use a local variable for the gsub result to avoid passing the substitution
-- count as a second argument to vim.json.decode (string.gsub returns two values).
local function find_cmd(cmd_name)
    for _, entry in ipairs(sent) do
        local stripped = entry.data:gsub("\n$", "")
        local ok, decoded = pcall(vim.json.decode, stripped)
        if ok and decoded.cmd == cmd_name then return decoded end
    end
    return nil
end

-- Switch the current window to a fresh scratch buffer and return its handle.
-- This ensures the viewer's gfx buffer can be cleanly wiped between tests
-- (bufhidden=wipe triggers when it leaves all windows).
local function fresh_buf()
    local b = vim.api.nvim_create_buf(true, true)
    vim.api.nvim_set_current_buf(b)
    return b
end

print("Test 1: open() starts a job and sends a show command")
reset_capture()
viewer.open("/tmp/fake.png")
assert_eq("  jobs_started count", #jobs_started, 1)
local show = find_cmd("show")
assert_true("  show command sent", show ~= nil)
if show then
    assert_eq("    show.path", show.path, "/tmp/fake.png")
end

print("Test 2: focus change (BufLeave/WinLeave) does NOT close the viewer")
reset_capture()
-- Dedicated viewer: merely changing focus (e.g. tabbing to the sidebar) must
-- leave the image up. Only opening another file or pressing q closes it.
vim.cmd("doautocmd BufLeave")
vim.cmd("doautocmd WinLeave")
assert_true("  no quit on focus change", find_cmd("quit") == nil)
assert_true("  viewer still active after focus change", viewer.is_active())
viewer.close()
fresh_buf()

print("Test 3: opening a different image replaces the viewer")
reset_capture()
viewer.open("/tmp/another.png")
assert_eq("  jobs_started count", #jobs_started, 1)
local show2 = find_cmd("show")
assert_true("  show command sent for new image", show2 ~= nil)
if show2 then
    assert_eq("    show.path", show2.path, "/tmp/another.png")
end

print("Test 4: close() sends quit")
reset_capture()
viewer.close()
local quit2 = find_cmd("quit")
assert_true("  quit command sent on close", quit2 ~= nil)

-- Prepare a clean starting buffer for Test 5.
fresh_buf()

print("Test 5: open() with previous open auto-closes first")
reset_capture()
viewer.open("/tmp/a.png")
local first_id = jobs_started[1].id
-- Switch back to a clean buffer so the gfx buf from the first open is not
-- current, then reopen — viewer should close the first job and start fresh.
fresh_buf()
reset_capture()
viewer.open("/tmp/b.png")
assert_eq("  jobs_started count after reopen", #jobs_started, 1)
assert_true("  new job_id differs from first", jobs_started[1].id ~= first_id)

-- Regression: NvimTree's open_in_new_window fires BufReadPost twice for one
-- file open. A duplicate open of the SAME path must be a no-op, not a
-- close-and-respawn (two renderers race on the shared image ID otherwise).
print("Test 6: duplicate open of same path does not respawn")
fresh_buf()
reset_capture()
viewer.open("/tmp/same.png")
assert_eq("  first open starts one job", #jobs_started, 1)
reset_capture()
viewer.open("/tmp/same.png")
assert_eq("  duplicate open starts no new job", #jobs_started, 0)
assert_true("  duplicate open sends no quit", find_cmd("quit") == nil)
-- Clean up so the teardown below doesn't see a live gfx buffer.
viewer.close()

-- Dedicated viewer: opening any other real file closes the image; media files
-- and special buffers (sidebar/nofile) do not.
print("Test 7: opening another real file closes the viewer")
fresh_buf()
reset_capture()
viewer.open("/tmp/img.png")
assert_true("  viewer active after open", viewer.is_active())
local foreign = vim.api.nvim_create_buf(true, false)   -- listed, normal buftype
vim.api.nvim_buf_set_name(foreign, "/tmp/notes.txt")
reset_capture()
viewer.on_other_buf(foreign)
vim.wait(50)   -- on_other_buf schedules M.close
assert_true("  quit sent when foreign file opened", find_cmd("quit") ~= nil)
assert_true("  viewer inactive after foreign open", not viewer.is_active())

print("Test 8: on_other_buf ignores media files and special buffers")
fresh_buf()
reset_capture()
viewer.open("/tmp/img2.png")
local media = vim.api.nvim_create_buf(true, false)
vim.api.nvim_buf_set_name(media, "/tmp/clip.mp4")
viewer.on_other_buf(media)
vim.wait(20)
assert_true("  media file does not close viewer", viewer.is_active())
local side = vim.api.nvim_create_buf(false, true)      -- scratch => buftype=nofile
viewer.on_other_buf(side)
vim.wait(20)
assert_true("  nofile/sidebar buffer does not close viewer", viewer.is_active())
viewer.close()

vim.fn.jobstart    = orig_jobstart
vim.fn.chansend    = orig_chansend
vim.fn.jobwait     = orig_jobwait
vim.fn.jobstop     = orig_jobstop
vim.fn.resolve     = orig_resolve
vim.fn.filereadable= orig_readable
vim.fn.executable  = orig_exec
vim.notify         = orig_notify

if failures == 0 then
    print("\nAll Lua tests passed.")
    vim.cmd("qa!")
else
    print(string.format("\n%d failure(s).", failures))
    vim.cmd("cq!")
end
