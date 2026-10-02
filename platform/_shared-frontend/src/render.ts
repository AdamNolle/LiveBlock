import type { PatchPayload } from "./ipc";

export function renderPatches(root: HTMLElement, patches: PatchPayload[]) {
  root.replaceChildren();
  for (const p of patches) {
    if (![p.x, p.y, p.width, p.height].every(Number.isFinite) || p.width <= 0 || p.height <= 0) continue;
    const patch = p.fill ? document.createElement("div") : document.createElement("img");
    patch.className = "patch";
    if (p.fill) {
      const channel = (n: number) => Number.isFinite(n) ? Math.min(255, Math.max(0, n)) : 0;
      patch.style.backgroundColor = `rgba(${channel(p.fill.r)}, ${channel(p.fill.g)}, ${channel(p.fill.b)}, ${channel(p.fill.a) / 255})`;
    } else (patch as HTMLImageElement).src = p.png_data_url;
    patch.style.left = `${p.x * 100}%`;
    patch.style.top = `${p.y * 100}%`;
    patch.style.width = `${p.width * 100}%`;
    patch.style.height = `${p.height * 100}%`;
    root.appendChild(patch);
  }
}
