import { logo } from "./primitives";
import { lb, events, type NormalizedRegion } from "./ipc";
import { el, errorText, point, rectangle, positionBox } from "./dom";
const stage = document.querySelector<HTMLElement>(".stage")!;
el("logo").appendChild(logo({ size: 32, radius: 8 }));
let regions: NormalizedRegion[] = [];
let start: [number, number] | null = null;
let preview: HTMLElement | null = null;
function render() {
  stage.replaceChildren();
  for (const region of regions) {
    const box = document.createElement("div"); box.className = "region-box";
    positionBox(box, region); stage.appendChild(box);
  }
}
async function refresh() { regions = await lb.listRegions(); render(); }
function fail(error: unknown) { el("markBtn").textContent = errorText(error); }
stage.onpointerdown = event => {
  if (event.button !== 0) return;
  start = point(event, stage); stage.setPointerCapture(event.pointerId);
  preview = document.createElement("div"); preview.className = "region-box"; stage.appendChild(preview);
};
stage.onpointermove = event => { if (start && preview) positionBox(preview, rectangle(start, point(event, stage))); };
stage.onpointerup = async event => {
  if (!start) return;
  const rect = rectangle(start, point(event, stage)); start = null;
  preview?.remove(); preview = null;
  if (rect.width < 0.003 || rect.height < 0.003) return;
  try { await lb.addRegion({ id: crypto.randomUUID(), ...rect }); await refresh(); }
  catch (error) { fail(error); }
};
stage.onpointercancel = () => { start = null; preview?.remove(); preview = null; };
el("cancelBtn").onclick = () => { void lb.hideWindow("editor").catch(fail); };
document.onkeydown = event => {
  if (event.key === "Escape") void lb.hideWindow("editor").catch(fail);
  if ((event.key === "Backspace" || event.key === "Delete") && regions.length) {
    event.preventDefault(); void lb.deleteRegion(regions[regions.length - 1].id).then(refresh).catch(fail);
  }
};
events.onRegions(value => { regions = value; render(); }).catch(fail);
void refresh().catch(fail);
