# CasaPic user guide

## First start

On the first start CasaPic opens **Tools ▸ Folder Manager**. Pick the folders that hold your
photos and choose, for each:

- **Scan Always**: CasaPic shows the folder and watches it, so new photos appear by themselves.
- **Scan Once**: CasaPic reads the folder now and does not watch it.
- **Remove**: CasaPic stops showing the folder. Nothing on disk changes.

You can come back to the Folder Manager at any time, or use **File ▸ Add Folder to CasaPic**.
**File ▸ Add File to CasaPic** (Ctrl+O) picks single photos: each photo's folder is added as
Scan Once and the first photo opens.

## The Library

The Library is the main window: folders and albums on the left, photos on the right, and the
Photo Tray along the bottom.

- **Folders** are grouped by year. The tree button above the list (or **View ▸ Folder View**)
  shows them as a tree instead. Right-click a folder to sort its photos, rename, move, hide or
  delete it, or to remove it from CasaPic.
- **Thumbnail size**: the slider in the tray, from small up to one photo per row. Ctrl+1 and
  Ctrl+2 give small and normal sizes.
- **Your place is kept.** Each folder, album, search and the timeline remembers where you were,
  so opening a photo and coming back, or switching folders and back, returns to the same spot.
  Clicking a folder in the list jumps to the start of that folder.
- **Selecting**: click, Ctrl+click and Shift+click as in Windows Explorer, or arrow keys with
  Shift. Ctrl+A selects everything, Ctrl+D clears the selection, Ctrl+I inverts it.
- **Hold in the Tray** (Ctrl+H) keeps photos in the tray while you look at other folders.

### Organising

| Task | How |
|---|---|
| Star a photo | Ctrl+8 (again to unstar), the star button in the tray, or Picture ▸ Star / Unstar |
| Tag photos | View ▸ Tags (Ctrl+T) |
| Caption a photo | type under the photo in the Edit view, or in Properties (Alt+Enter) |
| Make an album | File ▸ New Album (Ctrl+N), then Picture ▸ Add to Album, or drag photos onto it |
| Find photos | Edit ▸ Find (Ctrl+F): searches names, folders, tags, captions and album names |
| See everything by month | View ▸ Timeline (Ctrl+5) |
| Hide a photo | Picture ▸ Hide; View ▸ Hidden Pictures shows hidden ones again |

Tags and captions are written inside the photo file, where Picasa and most other photo programs
can read them. CasaPic keeps the untouched original first.

### Files

| Task | How |
|---|---|
| Rename | File ▸ Rename (F2) |
| Delete | File ▸ Delete from Disk (Delete key): the photo goes to the Windows Recycle Bin |
| Move or copy | drag to a folder (Ctrl+drag copies), or Cut / Copy / Paste (Ctrl+X / C / V) |
| Export a resized copy | File ▸ Export Picture to Folder (Ctrl+Shift+S) |
| Change the date taken | Picture ▸ Adjust Date and Time (also adds a date to a photo that has none) |
| Move a folder | Folder ▸ Move Folder (to another disk too: CasaPic copies it, checks the copy, then sends the old folder to the Recycle Bin) |
| Show in Windows Explorer | File ▸ Locate on Disk (Ctrl+Enter) |

**Delete without being asked each time.** The question before deleting has a "Don't ask again"
box. Choose "until I close CasaPic" or "always". To be asked again, tick "Ask before Delete
from Disk" in Tools ▸ Options ▸ General.

## Editing a photo

Double-click a photo (or press Enter) to open the Edit view. ← and → move to the previous and
next photo; Esc goes back to the Library.

The panel on the left has four tabs:

- **Basic** (Basic Fixes): Crop, Straighten, I'm Feeling Lucky, Auto Contrast, Auto Color, Fill Light.
- **Tuning**: Fill Light, Highlights, Shadows, Color Temperature, and the Neutral Color Picker.
- **Effects**: twelve effects such as Sharpen, Sepia, B&W, Warmify, Film Grain, Soft Focus and
  Graduated Tint.
- **Studio**: tools in the style of the Snapseed app (see below), and your Looks.

Every edit can be undone (Ctrl+Z) and redone (Ctrl+Y). **Before / After** shows the photo
with and without the edits side by side.

**Zoom**: the slider in the bar under the photo goes from Fit to 400%. Ctrl+mouse wheel zooms too.
When you are zoomed in, a small overview shows which part you are looking at; click or drag in
it to move around. The Fit and 100% buttons jump straight to those sizes.

### Saving

Edits are kept as a recipe beside the photo and shown everywhere in CasaPic, but the photo file
itself does not change until you press **Save** (Ctrl+S). Save writes the edited photo and
keeps the untouched original in a hidden `.casapicoriginals` folder; **Revert** brings it back.

### The Studio tab

| Tool | What it does |
|---|---|
| Tune Image | Brightness, Contrast, Saturation, Ambiance, Highlights, Shadows, Warmth |
| Details | Structure (brings out texture) and Sharpening |
| Vignette | darkens or lightens the edges; click the photo to move the centre |
| HDR Scape | a strong local-contrast look, with its own brightness and saturation |

The slider ranges follow the Snapseed app (−100 to +100). CasaPic draws these tools with its own
code, so results are close to Snapseed's but not identical.

### Looks: one set of edits for many photos

A **Look** is a named set of edits that you can put on any photo.

- **Save one**: edit a photo the way you like, open the Studio tab, and press
  **Save these edits as a Look…**. Colour and tone edits are kept; crop, rotation and
  straightening are not, since they belong to one photo.
- **Use one on this photo**: press its name in the Studio tab.
- **Use one on many photos**: select them in the Library, then **Picture ▸ Apply Look: (name)**.
  Each photo gets the same edits, and each can still be undone on its own.
- Rename (✎) or delete (✕) a Look in the Studio tab. Deleting a Look does not change photos that
  already have it.

CasaPic comes with one Look, **My Snapseed look**: a soft circular vignette that darkens the
edges and slightly lifts the centre.

Other ways to repeat edits: **Picture ▸ Copy All Effects** and **Paste All Effects** copy one
photo's edits to others, and **Picture ▸ Batch Edit** applies a single fix to every selected
photo.

## Viewing and sharing

- **Slideshow** (Ctrl+4): ← / → to move, Space to pause, Esc to stop. Its speed and fade are in
  Tools ▸ Options ▸ Slideshow.
- **Full screen** (F11).
- **Print** (Ctrl+P) and **Print Contact Sheet** (Ctrl+Shift+P).
- **Create ▸ Picture Collage**: six layouts, borders, background and captions; saved as a new
  picture.

## The quick photo viewer

When CasaPic is the program for a photo type, double-clicking such a photo in Windows Explorer
opens the **quick viewer** instead of the whole library: the photo over a dimmed screen. It
opens fast because it reads nothing but that folder's file names.

| Task | How |
|---|---|
| Next / previous photo in the folder | → / ← (or Page Down / Page Up), or the arrow buttons |
| First / last photo | Home / End |
| Zoom | mouse wheel, + and -, or the slider; **1:1** (key 1) for actual size, **Fit** (key 0) |
| Move around a zoomed photo | drag it |
| Slideshow | Space, or the round play button |
| Rotate | Ctrl+R clockwise, Ctrl+Shift+R counter-clockwise (kept as an edit, as in the library) |
| Star | Ctrl+8, or the star button |
| Delete | Delete key: the photo goes to the Windows Recycle Bin, after a question |
| Photo details | I, or the ⓘ button |
| Open it in the full CasaPic | **Edit in CasaPic** (Enter): the photo opens in the Edit view; a folder not yet in the library is added as Scan Once |
| Close | Esc, the ✕ button, or a click on the dimmed screen around the photo |

Stars and turns go into the folder's `.casapic.ini`, so the library shows them too.

**Making CasaPic the program for photos.** The installer offers CasaPic for JPEG, PNG, GIF, BMP,
TIFF, WebP and HEIC files. Windows decides the default program itself: right-click a photo ▸
**Open with ▸ Choose another app**, pick **CasaPic**, and tick **Always use this app**. (Or
Windows **Settings ▸ Apps ▸ Default apps ▸ CasaPic**.) Windows does not let a program make
itself the default.

## Settings

**Tools ▸ Options** holds the general settings, which file types to show, slideshow settings,
and **Colours**: Light, Dark, or Same as Windows.

**Updates.** CasaPic updates itself from its releases on GitHub: when the library opens it
checks for a newer version, and if there is one it downloads it, checks that it really comes
from CasaPic, installs it and opens again (a few seconds). While it stays open it checks once a
day and waits for you to choose **Restart and install**. **Tools ▸ Options ▸ Updates** shows the
version you have and the last check, has **Check now**, and a box to turn automatic updates
off. An update never touches your library or your photos.

Settings belong to each Windows user: every person who signs in to the computer has their own
folders, options, albums and thumbnail cache (in their own
`%LOCALAPPDATA%\org.casapic.app\library`). A library from an earlier CasaPic, kept in
`%LOCALAPPDATA%\CasaPic`, is copied there once at the first start; the old copy is left in
place as a backup and can be deleted (the files `index.db`, `index.db-wal`, `index.db-shm` and
`settings.json` in that folder).

All keyboard shortcuts are listed under **Help ▸ Keyboard Shortcuts** (F1).

## Moving from Picasa

**Tools ▸ Import Picasa Settings** reads the `.picasa.ini` files Picasa left in your photo
folders and brings across stars, edits, albums, hidden photos, captions and folder descriptions.
Picasa's files are only read, never changed. Any Picasa effect CasaPic cannot draw yet is kept
and listed, so nothing is lost.
