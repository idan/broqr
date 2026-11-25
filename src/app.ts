import type { Backend, LabelPreview } from "./backend";

type Tone = "info" | "success" | "warning" | "error";

const qrIcon = `<svg viewBox="0 0 32 32" fill="currentColor" aria-hidden="true">
  <path fill-rule="evenodd" d="M2 2h12v12H2zm3 3v6h6V5zM18 2h12v12H18zm3 3v6h6V5zM2 18h12v12H2zm3 3v6h6v-6z"/>
  <path d="M7 7h2v2H7zM23 7h2v2h-2zM7 23h2v2H7zM18 18h4v4h-4zM26 18h4v4h-4zM22 22h4v4h-4zM18 26h4v4h-4zM26 26h4v4h-4z"/>
</svg>`;

export function createApp(root: HTMLElement, backend: Backend, desktop: boolean) {
  root.innerHTML = `
    <main class="shell">
      <header class="app-header">
        <div class="brand"><span class="brand-icon">${qrIcon}</span><div><h1>BroQR</h1><p>Just the image. Just the label.</p></div></div>
        <span class="device-tag"><span class="connection-dot"></span>QL-810W <span class="tag-divider">/</span> 62 mm</span>
      </header>
      <div id="browser-notice" class="browser-notice" hidden>Browser preview only. Open the desktop app to choose images, access the clipboard, and print.</div>
      <section class="printer-card" aria-labelledby="printer-heading">
        <div class="section-heading"><div><h2 id="printer-heading">Your printer</h2><p>Direct over Wi-Fi. No print dialog.</p></div><span class="eyebrow">01 / CONNECT</span></div>
        <form id="printer-form">
          <label class="field-label" for="printer-address">Printer IP address</label>
          <div class="printer-fields">
            <input id="printer-address" name="printerAddress" type="text" placeholder="192.168.1.50" spellcheck="false" autocapitalize="off" autocomplete="off" aria-describedby="address-help" />
            <button id="save-button" class="button button-subtle" type="submit">Save</button>
            <button id="test-button" class="button button-outline" type="button">Test connection</button>
          </div>
          <p id="address-help" class="field-help">Saved on this computer. Use a DHCP reservation to keep this address stable.</p>
        </form>
      </section>
      <section class="image-card" aria-labelledby="image-heading">
        <div class="section-heading"><div><h2 id="image-heading">Your label</h2><p>The whole image, scaled to fit the tape.</p></div><span class="eyebrow">02 / PRINT</span></div>
        <div class="label-workspace">
          <div id="drop-zone" class="drop-zone">
            <div id="empty-state" class="empty-state">
              <span class="drop-icon">${qrIcon}</span>
              <h3>Drop an image here</h3>
              <p>QR code, extra text, white border.<br />Everything stays in the picture.</p>
              <span class="file-types">PNG · JPG · WEBP · BMP · GIF · TIFF</span>
            </div>
            <div id="preview-state" class="preview-state" hidden>
              <span class="preview-caption">EXACT PRINT ARTWORK</span>
              <img id="label-preview" alt="Black-and-white preview of the image that will be printed" />
            </div>
            <div id="drop-overlay" class="drop-overlay" hidden>Drop to replace the image</div>
          </div>
          <aside class="label-details" aria-label="Label details">
            <div class="image-actions">
              <button id="choose-button" class="button button-outline" type="button">Choose image<span class="key-hint">⌘ / Ctrl O</span></button>
              <button id="paste-button" class="button button-subtle" type="button">Paste image<span class="key-hint">⌘ / Ctrl V</span></button>
            </div>
            <div class="divider"></div>
            <p id="image-name" class="image-name">No image selected</p>
            <dl class="measurements">
              <div><dt>Source</dt><dd id="source-size">—</dd></div>
              <div><dt>Print pixels</dt><dd id="print-size">—</dd></div>
              <div><dt>Artwork</dt><dd id="artwork-size">—</dd></div>
              <div><dt>Cut length <span title="Estimate includes minimum 3 mm feed at each end. Actual cutter behavior may vary.">≈</span></dt><dd id="cut-length">—</dd></div>
            </dl>
            <div class="render-notes">
              <span>↗ Short edge fills the tape</span>
              <span>▦ Crisp nearest-neighbor scaling</span>
              <span>✂ Automatic cut after one label</span>
            </div>
            <p id="rotation-note" class="rotation-note" hidden>Rotated 90° to use the full printable width.</p>
          </aside>
        </div>
        <div class="print-bar">
          <div id="status" class="status" role="status" aria-live="polite" data-tone="info"><span id="status-dot" class="status-dot"></span><p id="status-text"></p></div>
          <button id="print-button" class="button button-primary" type="button">Print label<span class="print-arrow">↗</span></button>
        </div>
      </section>
      <footer><span>Local images. Direct printing. Nothing uploaded.</span><span>300 dpi · 62 mm continuous tape</span></footer>
      <p class="hardware-note">BroQR adds no page-layout padding. Existing image borders stay intact; the printer still needs its physical side and feed margins. Black-on-white tape only.</p>
    </main>
  `;

  const el = <T extends HTMLElement = HTMLElement>(id: string): T => {
    const element = root.querySelector<T>(`#${id}`);
    if (!element) throw new Error(`Missing element: ${id}`);
    return element;
  };
  const addressInput = el<HTMLInputElement>("printer-address");
  const printButton = el<HTMLButtonElement>("print-button");
  const previewImage = el<HTMLImageElement>("label-preview");
  const controls = ["save-button", "test-button", "choose-button", "paste-button"];
  const cleanup: Array<() => void> = [];
  let label: LabelPreview | null = null;
  let address = "";
  let busy = desktop;
  let busyLabel = "Starting…";
  let disposed = false;
  let message = desktop ? "Loading your printer settings…" : "Printing is available in the desktop app.";
  let tone: Tone = "info";

  function listen(target: EventTarget, event: string, listener: EventListener) {
    target.addEventListener(event, listener);
    cleanup.push(() => target.removeEventListener(event, listener));
  }

  function setStatus(text: string, nextTone: Tone = "info") {
    message = text;
    tone = nextTone;
  }

  function render() {
    if (disposed) return;
    el("browser-notice").hidden = desktop;
    addressInput.disabled = !desktop || busy;
    if (addressInput.value !== address) addressInput.value = address;
    for (const id of controls) el<HTMLButtonElement>(id).disabled = !desktop || busy;
    el<HTMLButtonElement>("test-button").disabled = !desktop || busy || !address.trim();
    printButton.disabled = !desktop || busy || !label || !address.trim();
    printButton.textContent = busy ? busyLabel : "Print label ↗";
    el("status").dataset.tone = tone;
    el("status").classList.toggle("is-busy", busy);
    el("status-text").textContent = message;
    el("drop-zone").classList.toggle("is-busy", busy);
    el("empty-state").hidden = label !== null;
    el("preview-state").hidden = label === null;
    el("image-name").textContent = label?.name ?? "No image selected";
    if (label) {
      if (previewImage.getAttribute("src") !== label.previewUrl) previewImage.src = label.previewUrl;
      const layout = label.layout;
      el("source-size").textContent = `${layout.sourceWidth} × ${layout.sourceHeight} px`;
      el("print-size").textContent = `${layout.widthDots} × ${layout.heightDots} dots`;
      el("artwork-size").textContent = `${layout.artworkWidthMm.toFixed(1)} × ${layout.artworkLengthMm.toFixed(1)} mm`;
      el("cut-length").textContent = `${layout.labelLengthMm.toFixed(1)} mm`;
      el("rotation-note").hidden = !layout.rotated;
    }
  }

  function acceptLabel(next: LabelPreview) {
    label = next;
    setStatus("Image ready. The preview is exactly what will be printed.", "success");
  }

  async function perform(activity: string, action: () => Promise<void>) {
    if (!desktop || busy || disposed) return;
    busy = true;
    busyLabel = activity;
    setStatus(activity);
    render();
    try {
      await action();
    } catch (error) {
      setStatus(error instanceof Error ? error.message : String(error), "error");
    } finally {
      busy = false;
      render();
    }
  }

  async function saveAddress() {
    address = addressInput.value.trim();
    if (!address) throw new Error("Enter your printer's IP address first.");
    await backend.saveSettings({ printerAddress: address });
  }

  const choose = () => perform("Opening image…", async () => {
    const path = await backend.chooseImage();
    if (path) acceptLabel(await backend.loadImage(path));
    else setStatus(label ? "Current image unchanged." : "No image selected.");
  });
  const paste = () => perform("Reading clipboard…", async () => acceptLabel(await backend.pasteImage()));
  const print = () => perform("Printing…", async () => {
    if (!label) throw new Error("Choose an image first.");
    await saveAddress();
    const result = await backend.printImage(label.id, address);
    setStatus(result.message, result.confirmed ? "success" : "warning");
  });

  listen(addressInput, "input", () => {
    address = addressInput.value;
    render();
  });
  listen(el("printer-form"), "submit", (event) => {
    event.preventDefault();
    void perform("Saving…", async () => {
      await saveAddress();
      setStatus("Printer address saved on this computer.", "success");
    });
  });
  listen(el("test-button"), "click", () => {
    void perform("Connecting…", async () => {
      await saveAddress();
      const result = await backend.testPrinter(address);
      setStatus(result.message, result.statusVerified ? "success" : "warning");
    });
  });
  listen(el("choose-button"), "click", () => { void choose(); });
  listen(el("paste-button"), "click", () => { void paste(); });
  listen(printButton, "click", () => { void print(); });
  listen(root, "dragover", (event) => event.preventDefault());
  listen(root, "drop", (event) => event.preventDefault());
  listen(document, "keydown", (event) => {
    const key = event as KeyboardEvent;
    if (!(key.metaKey || key.ctrlKey) || key.altKey || key.repeat) return;
    const editing = key.target instanceof HTMLElement
      && (key.target.matches("input, textarea") || key.target.isContentEditable);
    if (key.key.toLowerCase() === "o") {
      key.preventDefault();
      void choose();
    } else if (key.key.toLowerCase() === "v" && !editing) {
      key.preventDefault();
      void paste();
    } else if (key.key === "Enter" && !printButton.disabled) {
      key.preventDefault();
      void print();
    }
  });

  render();
  const ready = desktop ? backend.loadSettings().then((settings) => {
    address = settings.printerAddress;
    setStatus(address ? "Choose an image to get started." : "Add your printer's IP address to get started.");
  }).catch((error: unknown) => {
    setStatus(error instanceof Error ? error.message : String(error), "error");
  }).finally(() => {
    busy = false;
    render();
  }) : Promise.resolve();

  return {
    ready,
    setDropActive(active: boolean) {
      if (disposed) return;
      el("drop-overlay").hidden = !active || busy || !desktop;
      el("drop-zone").classList.toggle("is-dragging", active && !busy && desktop);
    },
    receivePaths(paths: string[]) {
      if (paths.length !== 1) {
        if (!busy) {
          setStatus("Drop one image at a time.", "warning");
          render();
        }
        return Promise.resolve();
      }
      return perform("Preparing image…", async () => acceptLabel(await backend.loadImage(paths[0])));
    },
    dispose() {
      disposed = true;
      for (const remove of cleanup) remove();
    },
  };
}
