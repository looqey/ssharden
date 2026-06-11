// IronRDP spike step 0: IPC throughput probe (docs/IRONRDP.md).
//
// When the app runs with SSHARDEN_PROBE=1, this streams synthetic RGBA frames from
// Rust over a raw ipc::Channel, paints each one to a canvas with putImageData, and
// reports throughput + paint timing to stdout via the probe_report command. It
// answers one question: is Channel→canvas fast enough on webkit2gtk for embedded RDP?

import { Channel, invoke } from "@tauri-apps/api/core";

interface ScenarioResult {
  label: string;
  frames: number;
  frameBytes: number;
  totalMs: number;
  fps: number;
  mbPerSec: number;
  avgPaintMs: number;
  maxPaintMs: number;
  dropped: number;
}

/** Frame payload layout (matches probe_frames in Rust): 16-byte LE header + RGBA. */
const HEADER_BYTES = 16;

async function runScenario(
  label: string,
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  count: number,
): Promise<ScenarioResult> {
  const frameBytes = HEADER_BYTES + width * height * 4;
  let received = 0;
  let paintTotal = 0;
  let paintMax = 0;
  let dropped = 0;

  const done = new Promise<void>((resolve) => {
    const channel = new Channel<ArrayBuffer | number[]>();
    channel.onmessage = (raw) => {
      const buf = raw instanceof ArrayBuffer ? new Uint8Array(raw) : Uint8Array.from(raw);
      const head = new DataView(buf.buffer, buf.byteOffset, HEADER_BYTES);
      const seq = head.getUint32(0, true);
      const w = head.getUint32(4, true);
      const h = head.getUint32(8, true);
      if (buf.byteLength !== HEADER_BYTES + w * h * 4) {
        dropped++;
      } else {
        const t = performance.now();
        const pixels = new Uint8ClampedArray(buf.buffer, buf.byteOffset + HEADER_BYTES, w * h * 4);
        ctx.putImageData(new ImageData(pixels, w, h), 0, 0);
        const dt = performance.now() - t;
        paintTotal += dt;
        if (dt > paintMax) paintMax = dt;
      }
      received++;
      if (seq === count - 1 || received === count) resolve();
    };
    void invoke("probe_frames", { channel, width, height, count });
  });

  const t0 = performance.now();
  await done;
  const totalMs = performance.now() - t0;
  return {
    label,
    frames: received,
    frameBytes,
    totalMs: Math.round(totalMs),
    fps: Math.round((received / totalMs) * 1000),
    mbPerSec: Math.round(((received * frameBytes) / 1e6 / totalMs) * 1000),
    avgPaintMs: Math.round((paintTotal / Math.max(received, 1)) * 100) / 100,
    maxPaintMs: Math.round(paintMax * 100) / 100,
    dropped,
  };
}

/**
 * Run the probe and report to stdout if SSHARDEN_PROBE=1; returns whether it ran.
 * Shows the frames on a fullscreen canvas so a stalled transport is visible.
 */
export async function maybeRunProbe(): Promise<boolean> {
  const enabled = await invoke<boolean>("probe_enabled").catch(() => false);
  if (!enabled) return false;

  const canvas = document.createElement("canvas");
  canvas.width = 1280;
  canvas.height = 800;
  canvas.style.cssText = "position:fixed;inset:0;z-index:9999;background:#000";
  document.body.appendChild(canvas);
  const ctx = canvas.getContext("2d")!;

  const results: ScenarioResult[] = [];
  // Dirty-rect-sized frames: the steady-state RDP case.
  results.push(await runScenario("small-64KB(128x128)x300", ctx, 128, 128, 300));
  // Full-frame repaints: the worst case (session open, full-screen change).
  results.push(await runScenario("full-4MB(1280x800)x30", ctx, 1280, 800, 30));

  await invoke("probe_report", { report: JSON.stringify(results) });
  canvas.remove();
  return true;
}
