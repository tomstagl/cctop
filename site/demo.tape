# VHS tape (https://github.com/charmbracelet/vhs) — the "live" recording for
# people who have vhs installed: `vhs site/demo.tape`. `make demo` does not
# require it; it renders frames headlessly instead.
Output site/assets/demo-vhs.gif
Set Shell "bash"
Set FontSize 13
Set Width 1280
Set Height 640
Set Theme "Catppuccin Mocha"
Type "CCTOP_FAKE_NOW=1787824000000 cctop run --session fixtures/session-a"
Enter
Sleep 2s
Type "Tab"
Sleep 1s
Type "Enter"
Sleep 2s
Escape
Type "i"
Sleep 2s
Escape
Type "q"
