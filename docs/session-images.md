# Session image pane

Click **Images** in the chat footer to move image previews into a dedicated pane
for that session. Inline display remains the default. The count includes images
read by the model and images attached to your messages, including loaded history.

- The pane initially follows the latest image. Click a thumbnail or an image link
  in the transcript to inspect an earlier image, then **Follow latest** to resume.
- The pane loads the selected preview and the thumbnails in view. Scroll the
  filmstrip with the wheel or trackpad to load more without switching sessions.
  Browsing does not send the images to a model.
- Focus the filmstrip by clicking a thumbnail or by pressing Tab until its
  outline appears. Press **←** or **→** to select an adjacent image, or **Esc**
  to close the pane and return to the composer without editing the draft.
- Click the main image or an image ID in the history list to open the full-window
  zoom and pan viewer. Empty space around the image does not open it.
  Escape closes the viewer and leaves the pane open.
- Click **Inline ×** or **Images** again to return to inline previews.
- Each session remembers its own display mode across UI reload and workspace
  recovery. Reload resumes following the latest image.
- On narrow panels the image pane stacks below the chat instead of squeezing it.

Pane mode leaves compact, clickable image references at their original transcript
positions, preserving the relationship between tool reads and model responses.
Images with unsupported or unavailable previews remain listed with a fallback.

## Verification

```sh
justrust test -p jcode-desktop-ui image_pane
python3 scripts/gallery-navigation-acceptance.py --run target/native-gallery-proof
python3 scripts/screenshot.py target/session-images.png --transcript image --image-pane-interact
```

The macOS check uses a hidden native window with 96 offline images and two
sessions. It saves screenshots and the observed scroll, focus, and preview
states. The Linux screenshot check uses a private Xvfb display.
