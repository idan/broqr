import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

export interface Settings {
  printerAddress: string;
}

export interface LabelLayout {
  sourceWidth: number;
  sourceHeight: number;
  widthDots: number;
  heightDots: number;
  rotated: boolean;
  artworkWidthMm: number;
  artworkLengthMm: number;
  labelLengthMm: number;
}

export interface LabelPreview {
  id: number;
  name: string;
  layout: LabelLayout;
  previewUrl: string;
}

export interface PrinterCheck {
  statusVerified: boolean;
  message: string;
}

export interface PrintOutcome {
  confirmed: boolean;
  bytesSent: number;
  message: string;
}

export interface Backend {
  loadSettings(): Promise<Settings>;
  saveSettings(settings: Settings): Promise<void>;
  chooseImage(): Promise<string | null>;
  loadImage(path: string): Promise<LabelPreview>;
  pasteImage(): Promise<LabelPreview>;
  testPrinter(address: string): Promise<PrinterCheck>;
  printImage(id: number, address: string): Promise<PrintOutcome>;
}

export const nativeBackend: Backend = {
  loadSettings: () => invoke("load_settings"),
  saveSettings: (settings) => invoke("save_settings", { settings }),
  chooseImage: async () => {
    const path = await open({
      multiple: false,
      directory: false,
      title: "Choose an image to print",
      filters: [{
        name: "Images",
        extensions: ["png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff"],
      }],
    });
    return typeof path === "string" ? path : null;
  },
  loadImage: (path) => invoke("load_image", { path }),
  pasteImage: () => invoke("paste_image"),
  testPrinter: (address) => invoke("test_printer", { address }),
  printImage: (id, address) => invoke("print_image", { id, address }),
};
