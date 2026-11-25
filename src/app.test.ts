import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createApp } from "./app";
import type { Backend, LabelPreview } from "./backend";

const preview: LabelPreview = {
  id: 42,
  name: "return-code.png",
  previewUrl: "data:image/png;base64,AA==",
  layout: {
    sourceWidth: 800, sourceHeight: 1000, widthDots: 696, heightDots: 870,
    rotated: false, artworkWidthMm: 58.928, artworkLengthMm: 73.66,
    labelLengthMm: 79.5867,
  },
};

function backendMock(): Backend {
  return {
    loadSettings: vi.fn().mockResolvedValue({ printerAddress: "192.168.1.50" }),
    saveSettings: vi.fn().mockResolvedValue(undefined),
    chooseImage: vi.fn().mockResolvedValue("/tmp/return-code.png"),
    loadImage: vi.fn().mockResolvedValue(preview),
    pasteImage: vi.fn().mockResolvedValue({ ...preview, id: 43, name: "Clipboard image" }),
    testPrinter: vi.fn().mockResolvedValue({ statusVerified: true, message: "Ready." }),
    printImage: vi.fn().mockResolvedValue({ confirmed: true, bytesSent: 123, message: "Completed." }),
  };
}

describe("desktop app", () => {
  let root: HTMLElement;
  let backend: Backend;
  let app: ReturnType<typeof createApp>;

  const button = (id: string) => root.querySelector<HTMLButtonElement>(`#${id}`)!;
  const text = (id: string) => root.querySelector(`#${id}`)!.textContent;
  const idle = async () => {
    await vi.waitFor(() => expect(button("choose-button").disabled).toBe(false));
  };

  beforeEach(async () => {
    root = document.createElement("div");
    document.body.append(root);
    backend = backendMock();
    app = createApp(root, backend, true);
    await app.ready;
  });

  afterEach(() => {
    app.dispose();
    root.remove();
  });

  it("restores the printer address without automatically printing", () => {
    expect(root.querySelector<HTMLInputElement>("#printer-address")!.value).toBe("192.168.1.50");
    expect(button("print-button").disabled).toBe(true);
    expect(backend.printImage).not.toHaveBeenCalled();
  });

  it("loads the entire image and shows the backend's exact preview and dimensions", async () => {
    button("choose-button").click();
    await idle();
    expect(backend.loadImage).toHaveBeenCalledWith("/tmp/return-code.png");
    expect(text("print-size")).toBe("696 × 870 dots");
    expect(text("cut-length")).toBe("79.6 mm");
    expect(root.querySelector("img")!.getAttribute("src")).toBe(preview.previewUrl);
    expect(button("print-button").disabled).toBe(false);
    expect(backend.printImage).not.toHaveBeenCalled();
  });

  it("prints only on request using the cached preview ID and saves the address", async () => {
    await app.receivePaths(["/tmp/image.png"]);
    button("print-button").click();
    await idle();
    expect(backend.saveSettings).toHaveBeenCalledWith({ printerAddress: "192.168.1.50" });
    expect(backend.printImage).toHaveBeenCalledExactlyOnceWith(42, "192.168.1.50");
    expect(text("status-text")).toBe("Completed.");
  });

  it("prevents overlapping jobs from rapid clicks and keyboard shortcuts", async () => {
    await app.receivePaths(["/tmp/image.png"]);
    let finish!: (result: { confirmed: boolean; bytesSent: number; message: string }) => void;
    vi.mocked(backend.printImage).mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    button("print-button").click();
    button("print-button").click();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", ctrlKey: true }));
    await vi.waitFor(() => expect(backend.printImage).toHaveBeenCalledTimes(1));
    expect(button("choose-button").disabled).toBe(true);
    finish({ confirmed: true, bytesSent: 1, message: "Done." });
    await idle();
  });

  it("does not claim printing succeeded when the printer did not confirm it", async () => {
    vi.mocked(backend.printImage).mockResolvedValue({
      confirmed: false, bytesSent: 123, message: "Sent; check the printer before retrying.",
    });
    await app.receivePaths(["/tmp/image.png"]);
    button("print-button").click();
    await idle();
    expect(root.querySelector<HTMLElement>("#status")!.dataset.tone).toBe("warning");
    expect(text("status-text")).toContain("check the printer");
    expect(backend.printImage).toHaveBeenCalledTimes(1);
  });

  it("shows errors and never automatically retries a failed job", async () => {
    vi.mocked(backend.printImage).mockRejectedValue("Printer: cover is open.");
    await app.receivePaths(["/tmp/image.png"]);
    button("print-button").click();
    await idle();
    expect(text("status-text")).toContain("cover is open");
    expect(root.querySelector<HTMLElement>("#status")!.dataset.tone).toBe("error");
    expect(backend.printImage).toHaveBeenCalledTimes(1);
  });

  it("pastes from the clipboard but leaves normal text-field paste alone", async () => {
    const input = root.querySelector<HTMLInputElement>("#printer-address")!;
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "v", ctrlKey: true, bubbles: true }));
    expect(backend.pasteImage).not.toHaveBeenCalled();
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "v", ctrlKey: true }));
    await idle();
    expect(backend.pasteImage).toHaveBeenCalledTimes(1);
    expect(text("image-name")).toBe("Clipboard image");
  });

  it("rejects multiple dropped files rather than printing or silently picking one", async () => {
    await app.receivePaths(["one.png", "two.png"]);
    expect(backend.loadImage).not.toHaveBeenCalled();
    expect(text("status-text")).toContain("one image at a time");
  });

  it("renders filenames as text rather than HTML", async () => {
    vi.mocked(backend.loadImage).mockResolvedValue({ ...preview, name: '<img src=x onerror="alert(1)">' });
    await app.receivePaths(["/tmp/image.png"]);
    expect(text("image-name")).toBe('<img src=x onerror="alert(1)">');
    expect(root.querySelector("#image-name img")).toBeNull();
  });

  it("does not submit a print job if saving/validating the address fails", async () => {
    await app.receivePaths(["/tmp/image.png"]);
    vi.mocked(backend.saveSettings).mockRejectedValue("Invalid printer address.");
    button("print-button").click();
    await idle();
    expect(text("status-text")).toContain("Invalid printer address");
    expect(backend.printImage).not.toHaveBeenCalled();
  });

  it("disables printing when the address is cleared", async () => {
    await app.receivePaths(["/tmp/image.png"]);
    const input = root.querySelector<HTMLInputElement>("#printer-address")!;
    input.value = "";
    input.dispatchEvent(new Event("input"));
    expect(button("print-button").disabled).toBe(true);
    expect(button("test-button").disabled).toBe(true);
  });

  it("tests the printer connection without submitting a label", async () => {
    button("test-button").click();
    await idle();
    expect(backend.testPrinter).toHaveBeenCalledExactlyOnceWith("192.168.1.50");
    expect(backend.printImage).not.toHaveBeenCalled();
  });

  it("shows the rotation notice for landscape images", async () => {
    vi.mocked(backend.loadImage).mockResolvedValue({
      ...preview, layout: { ...preview.layout, rotated: true, sourceWidth: 1000, sourceHeight: 800 },
    });
    await app.receivePaths(["landscape.png"]);
    expect(root.querySelector<HTMLElement>("#rotation-note")!.hidden).toBe(false);
  });

  it("does not invoke native functionality in browser-preview mode", async () => {
    app.dispose();
    backend = backendMock();
    app = createApp(root, backend, false);
    await app.ready;
    button("choose-button").click();
    await app.receivePaths(["image.png"]);
    expect(backend.loadSettings).not.toHaveBeenCalled();
    expect(backend.loadImage).not.toHaveBeenCalled();
    expect(backend.chooseImage).not.toHaveBeenCalled();
    expect(root.querySelector<HTMLElement>("#browser-notice")!.hidden).toBe(false);
  });
});
