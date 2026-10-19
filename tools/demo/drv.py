import subprocess, time, os, sys
os.environ["DISPLAY"] = ":99"
S = os.environ.get("GENIX_DEMO_DIR", "/tmp/genix_demo")
T0 = [None]
CAPS = []  # (start, end, text)


def sh(*a):
    subprocess.run(list(a), check=False)


def now():
    return 0.0 if T0[0] is None else time.time() - T0[0]


def start_clock():
    T0[0] = time.time()


def wait(t):
    time.sleep(t)


def move(x, y, steps=0):
    if steps:
        # плавное движение курсора, чтобы в записи было видно, куда идёт указатель
        out = subprocess.run(["xdotool", "getmouselocation", "--shell"], capture_output=True, text=True).stdout
        cur = dict(l.split("=") for l in out.split() if "=" in l)
        cx, cy = int(cur.get("X", 0)), int(cur.get("Y", 0))
        for i in range(1, steps + 1):
            sh("xdotool", "mousemove", str(int(cx + (x - cx) * i / steps)), str(int(cy + (y - cy) * i / steps)))
            time.sleep(0.012)
    sh("xdotool", "mousemove", str(x), str(y))


def click(x, y, pause=0.8, smooth=True):
    move(x, y, 14 if smooth else 0)
    time.sleep(0.15)
    sh("xdotool", "click", "1")
    time.sleep(pause)


def dbl(x, y, pause=1.0):
    move(x, y, 10)
    sh("xdotool", "click", "--repeat", "2", "--delay", "90", "1")
    time.sleep(pause)


def rclick(x, y, pause=0.8):
    move(x, y, 10)
    sh("xdotool", "click", "3")
    time.sleep(pause)


def key(k, pause=0.4):
    sh("xdotool", "key", k)
    time.sleep(pause)


def typ(s, pause=0.3):
    sh("xdotool", "type", "--delay", "70", "--", s)
    time.sleep(pause)


def drag(x0, y0, x1, y1, button=1, steps=30, pause=0.6):
    move(x0, y0, 8)
    sh("xdotool", "mousedown", str(button))
    for i in range(1, steps + 1):
        sh("xdotool", "mousemove", str(int(x0 + (x1 - x0) * i / steps)), str(int(y0 + (y1 - y0) * i / steps)))
        time.sleep(0.02)
    sh("xdotool", "mouseup", str(button))
    time.sleep(pause)


def shot(name, crop=None):
    cmd = f"xwd -root | convert xwd:- "
    if crop:
        cmd += f"-crop {crop} +repage "
    cmd += f"{S}/{name}.png"
    subprocess.run(cmd, shell=True)


def say(text, dur=4.0):
    CAPS.append((now(), now() + dur, text))


def write_srt(path):
    def f(t):
        h, m, s = int(t // 3600), int(t % 3600 // 60), t % 60
        return f"{h:02d}:{m:02d}:{s:06.3f}".replace(".", ",")
    with open(path, "w", encoding="utf-8") as fh:
        for i, (a, b, t) in enumerate(CAPS, 1):
            fh.write(f"{i}\n{f(a)} --> {f(b)}\n{t}\n\n")


# ---- диалог параллелепипеда
F = {"w": 213, "d": 242, "h": 271, "x": 319, "y": 348, "z": 377}
FX = 1275


def setf(name, val):
    click(FX, F[name], pause=0.15)
    key("ctrl+a", 0.1)
    typ(str(val), 0.15)
    key("Return", 0.25)


def build_menu(item_y, pause=1.2):
    click(105, 124, pause=0.7)
    click(140, item_y, pause=pause)


OPY = {"new": 437, "join": 468, "cut": 497, "int": 526}


def box(w, d, h, x, y, z, op="new", confirm=True):
    build_menu(367)
    for n, v in zip("wdhxyz", (w, d, h, x, y, z)):
        setf(n, v)
    if op != "new":
        click(1280, 406, pause=0.6)
        click(1260, OPY[op], pause=0.6)
    if confirm:
        click(1134, 452, pause=1.2)


def focus():
    out = subprocess.run(["xdotool", "search", "--name", "Genix"], capture_output=True, text=True).stdout.split()
    if out:
        sh("xdotool", "windowfocus", out[0])


def launch(extra_env=None):
    sh("pkill", "genix")
    time.sleep(1)
    env = dict(os.environ, XDG_CONFIG_HOME=f"{S}/cH", GENIX_SETTINGS_PATH=f"{S}/set.json", GENIX_SKIP_LOGIN="1")
    if os.path.exists(f"{S}/set.json"):
        os.remove(f"{S}/set.json")
    if extra_env:
        env.update(extra_env)
    subprocess.Popen(["/home/user/genix/target/release/genix"], env=env, stdout=open(f"{S}/app.log", "w"), stderr=subprocess.STDOUT)
    time.sleep(6)
    focus()


def new_project():
    click(160, 145, pause=0.5)
    for i in range(8):
        move(500 + i * 9, 520)
        time.sleep(0.45)
    click(150, 20, pause=1.5)  # вкладка нового проекта
    focus()
