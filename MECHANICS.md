# Mechanics sheet — classic disk space map (from SpaceMonger.exe 1.4.0.1, registry key "1.3")

Original Windows program: **SpaceMonger 1.x** by **Sean Werkema** (Copyright © 1998, 1999, and 2000). See app `README.md` § Credits.

All addresses are in the original 32-bit Windows program. Values were read from its
disassembly; anything marked **estimated** was looked for but not found in the code.

## Settings and defaults (defaults set at 0x408b40, loaded from the registry at 0x408bb0)

| Setting | Default | Notes |
|---|---|---|
| density | 0 | combo box of 6 entries, selection = density + 3 (0x407c1e) → range -3..+2 |
| file_color | 0 (Rainbow) | list: Rainbow, Windows Colors, White, Light Gray, Dark Gray, Red, Orange, Yellow, Green, Aqua, Blue, Violet (table 0x42c1ec) |
| folder_color | 0 (Rainbow) | same list |
| auto_rescan (on delete) | 0 | |
| animated_zoom | 1 | |
| disable_delete | 0 | |
| show_rollover_box | 0 | |
| bias | 0 | trackbar 0..40, position = bias + 20 (0x407fc9) → range -20..+20; labels Horz / Equal / Vert |
| save_pos | 0 | |
| show_name_tips | 1 | nametip_delay = 125 ms (0x7d) |
| show_info_tips | 1 | infotip_delay = 250 ms (0xfa), infotip_flags = 0x1c |

Density labels in order (index 0..5): Too Few Files, Very Few Files, Normal, Lots of Files,
Very Many Files, Too Many Files. Default index 3 = "Lots of Files".

## Minimum box size by density (table 0x4249b8, pairs of (min width, min height))

| density | index | min w | min h |
|---|---|---|---|
| -3 | 0 | 96 | 64 |
| -2 | 1 | 64 | 48 |
| -1 | 2 | 48 | 32 |
| 0 | 3 | 32 | 24 |
| +1 | 4 | 24 | 16 |
| +2 | 5 | 16 | 12 |

A region is "big enough" only when width > min w AND height > min h (strict).

## Layout (0x4059b0 → 0x405a40)

**kSpaceMonger 0.3+:** the view-root folder itself fills the map as the outer frame; its
children are laid out in the inset (x+3, y+12, …). The original filled the map with the
view root’s *children* only (folder chrome disappeared on zoom).

1. *(Original)* The view root's children fill the whole map area: rect (0, 0, w-1, h-1), depth = the
   zoomed folder's own depth (0x40588f), so colours stay put when zooming.
2. Items of size 0 are skipped; the "<Free Space>" item is skipped unless Show Free Space is on.
3. Greedy two-way split: walk the items in order; add an item to group A unless
   sum(A) > sum(B), in which case it goes to group B (0x405aab).
   Items are taken largest first (**estimated**: no sort call was traced).
4. Direction: with bias b, wf = 8 + max(b,0), hf = 8 + max(-b,0). If wf*w/8 > hf*h/8 the
   width is cut (A left, B right), else the height (A top, B bottom) (0x405b63).
   Cut position = length * sum(A) / (sum(A)+sum(B)) using 64-bit math (0x405bc4).
5. For each group with its rect:
   * more than one item and rect big enough → recurse (step 3);
   * rect not big enough → one plain gray filler box (colour index -1) (0x405d2f);
   * exactly one item → place it; if it is a folder, its children are laid out in
     (x+3, y+12, w-6, h-15) at depth+1 (0x405cda).

## Drawing one box (0x404d4c)

* Skip boxes 2 px or less wide or tall.
* 1 px black outline (black brush 0x403291) on the box rect.
* 1 px bevel inside it: light colour top and left, dark colour bottom and right.
* Folder: fill colour frame at inset 2 and a header strip (x+3, y+3) to (x+w-3, y+12),
  9 px tall; children are drawn inside. File: fill rect at inset 2.
* Colour index = depth & 7; free space and gray fillers use the Windows button-face gray
  for fill, light and dark (flat).
* Rollover box on (show_rollover_box) and pointer over a box: dark ← fill, fill ← light,
  light ← white (0x404e79).
* Label font: "Small Fonts", height -9 px, weight 400 (0x4032b7). We use a clear sans
  font at about 10 px instead (the original bitmap font is not available on Linux).
* File label: name centred horizontally when it fits in w-2, else left at x+2; centred
  vertically when it fits; folders: left at x+2, top at y+1 (0x405128).
* Size line under a file name when the box is at least 48 wide and 36 tall (0x4040f3).

## Colour tables (fill / light / dark), RGB

Rainbow by depth (0x42c0c0, 0x42c0e0, 0x42c100):

| depth&7 | fill | light | dark |
|---|---|---|---|
| 0 | 255,127,127 | 255,191,191 | 191,127,127 |
| 1 | 255,191,127 | 255,223,191 | 191,159,95 |
| 2 | 255,255,0 | 255,255,191 | 191,191,63 |
| 3 | 127,255,127 | 191,255,191 | 127,191,127 |
| 4 | 127,255,255 | 223,255,255 | 127,191,191 |
| 5 | 191,191,255 | 223,223,255 | 159,159,255 |
| 6 | 191,191,191 | 223,223,223 | 159,159,159 |
| 7 | 255,127,255 | 255,191,255 | 191,127,191 |

Solid choices (fill 0x42c120[i], light 0x42c148[i], dark 0x42c170[i]):

| choice | fill | light | dark |
|---|---|---|---|
| White | 255,255,255 | 255,255,255 | 191,191,191 |
| Light Gray | 191,191,191 | 255,255,255 | 127,127,127 |
| Dark Gray | 127,127,127 | 191,191,191 | 63,63,63 |
| Red | 255,127,127 | 255,159,159 | 191,127,127 |
| Orange | 255,191,127 | 255,223,191 | 191,159,159 |
| Yellow | 255,255,0 | 255,255,191 | 191,191,63 |
| Green | 127,255,127 | 191,255,191 | 127,191,127 |
| Aqua | 127,255,255 | 223,255,255 | 127,191,191 |
| Blue | 191,191,255 | 223,223,255 | 159,159,255 |
| Violet | 255,127,255 | 255,191,255 | 191,127,191 |

Windows Colors: button face 192,192,192 / highlight 255,255,255 / shadow 128,128,128
(GetSysColor 15, 20, 16; classic Windows values, **estimated** for Linux).

## Commands (toolbar bitmap 200: 10 buttons, 44×15 px each; menu strings 0x42c2a8)

Open, Reload, Zoom Full, Zoom In, Zoom Out, Free Space, Run or Open, Delete, Setup, About.
Context menu: Zoom In, Zoom Out, Zoom Full, Run / Open, Delete, Open Drive..., Rescan Drive,
Show Free Space, Properties...

## Text formats

* Sizes: "%lu.%01lu %s" with units bytes / Kb / Mb / Gb (0x42c294); divisor 1024
  (0x404a10 compares with 0x400, 0x100000, 0x40000000). Tb added for modern disks (addition).
* Free-space box (flag 2, 0x405194): "<Free Space: %d.%d%%>", then the free size with "Free",
  "Files Total:  %u", "Folders Total:  %u" (strings 0x42c2a4, 0x42c2d4, 0x42c2d8, 0x42c2dc,
  "  %u" at 0x424b18). Exact wording of the size line is **estimated**.
* Percent: "%lu.%01lu%%"; free space item: "<Free Space: %d.%d%%>".
* Scan dialog: "Scanning Disk...", "Folders Found:", "Files Found:".
* Status: "Folders Total:", "Files Total:", "Total", "Free".

## Navigation

* Click selects a box (selected box's tip uses black background, white text, 0x404102).
* Zoom In makes the selected folder (or the folder holding the selected file) the view root.
* Zoom Out goes to the parent; Zoom Full back to the scanned root.
* Double-click: folder → Zoom In; file → Run / Open (**estimated** from command set).
* Animated zoom (0x406110): on by default. On the window DC with SetROP2(R2_NOT) it draws an
  inverted outline rectangle 8 times, Sleep(25 ms) between steps, moving from the folder's box
  to the whole map (zoom in) or the reverse (zoom out), over the old picture; then the new
  view is painted. We draw a black/white outline instead of a pixel inversion.
