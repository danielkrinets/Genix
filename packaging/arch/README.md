# Genix для Arch Linux

Два пакета, ставятся одной командой `makepkg -si` в папке пакета:

| Пакет | Что делает | Когда брать |
|---|---|---|
| `genix-bin` | ставит готовую сборку из «Releases» | быстро, без компилятора |
| `genix-git` | собирает из исходников последней версии | нужна свежая правка или своя архитектура (`aarch64`) |

```bash
git clone https://github.com/danielkrinets/Genix.git
cd Genix/packaging/arch/genix-bin     # или genix-git
makepkg -si
```

Нужна Vulkan-видеокарта: `vulkan-radeon` (AMD), `vulkan-intel` (Intel) или `nvidia-utils` (NVIDIA).
Удалить: `sudo pacman -R genix-bin` (или `genix-git`).

Отчёты о сбоях лежат в `~/.config/genix/crashes/` — их можно приложить через «Пожаловаться на баг» в боковой панели.

Для AUR: `makepkg --printsrcinfo > .SRCINFO`, затем обычная публикация пакета.
