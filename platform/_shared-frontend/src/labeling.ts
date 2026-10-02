import { wordmark } from "./primitives";
import { lb, type LabelDocument, type LabelBox, type ScreenshotEntry } from "./ipc";
import { el, point, rectangle, positionBox, errorText } from "./dom";
el("brand").appendChild(wordmark(16));
let shots: ScreenshotEntry[] = [];
let index = 0;
let doc: LabelDocument | null = null;
let loadedPath: string | null = null;
let objectURL: string | null = null;
let loadGeneration = 0;
let saveQueue: Promise<void> = Promise.resolve();
let start: [number, number] | null = null;
let preview: HTMLElement | null = null;
const stage = document.createElement("div"); stage.id = "imageStage";
const image = document.createElement("img"); image.draggable = false;
stage.appendChild(image); el("canvas").appendChild(stage);
function fail(error: unknown) { el("status").textContent = errorText(error); }
function render() {
  stage.querySelectorAll(".label-box").forEach(box => box.remove());
  for (const label of doc?.boxes ?? []) {
    const box = document.createElement("div"); box.className = `label-box ${label.class?.endsWith("keep") ? "keep" : ""}`;
    positionBox(box, label); stage.appendChild(box);
  }
}
async function load() {
  const generation = ++loadGeneration;
  doc = null; loadedPath = null; render();
  const shot = shots[index];
  if (!shot) { image.removeAttribute("src"); el("status").textContent = "No screenshots yet — start capture and take a screenshot"; return; }
  const [bytes, existing] = await Promise.all([lb.loadScreenshot(shot.path), lb.loadLabel(shot.path)]);
  if (generation !== loadGeneration) return;
  if (objectURL) URL.revokeObjectURL(objectURL);
  objectURL = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: "image/png" }));
  image.src = objectURL;
  await image.decode();
  if (generation !== loadGeneration) return;
  doc = existing ?? { image: `${shot.stem}.png`, imageWidth: image.naturalWidth, imageHeight: image.naturalHeight, boxes: [], labeledAt: new Date().toISOString() };
  loadedPath = shot.path;
  el("status").textContent = `${index + 1}/${shots.length} · ${shot.stem}`; render();
}
async function refresh() { shots = await lb.listScreenshots(); index = Math.min(index, Math.max(0, shots.length - 1)); await load(); }
async function save() {
  if (!doc || !loadedPath) return;
  doc.labeledAt = new Date().toISOString();
  const path = loadedPath; const snapshot = structuredClone(doc);
  const pending = saveQueue.catch(() => {}).then(() => lb.saveLabel(path, snapshot));
  saveQueue = pending; await pending;
}
stage.onpointerdown = event => {
  if (!doc || event.button !== 0) return;
  start = point(event, stage); stage.setPointerCapture(event.pointerId);
  preview = document.createElement("div"); preview.className = "label-box"; stage.appendChild(preview);
};
stage.onpointermove = event => { if (start && preview) positionBox(preview, rectangle(start, point(event, stage))); };
stage.onpointerup = event => {
  if (!start || !doc) return;
  const rect = rectangle(start, point(event, stage)); start = null; preview?.remove(); preview = null;
  if (rect.width < 0.003 || rect.height < 0.003) return;
  doc.boxes.push({ id: crypto.randomUUID(), ...rect, class: el<HTMLSelectElement>("labelClass").value as LabelBox["class"] }); render();
  void save().catch(fail);
};
stage.onpointercancel = () => { start = null; preview?.remove(); preview = null; };
el("captureBtn").onclick = async () => {
  try {
    await save();
    const path = await lb.captureScreenshotForLabeling();
    if (!path) throw new Error("Start capture before taking a screenshot.");
    await refresh(); index = Math.max(0, shots.findIndex(shot => shot.path === path)); await load();
  } catch (error) { fail(error); }
};
el("nextBtn").onclick = async () => { try { await save(); index = Math.min(index + 1, shots.length - 1); await load(); } catch (error) { fail(error); } };
el("prevBtn").onclick = async () => { try { await save(); index = Math.max(0, index - 1); await load(); } catch (error) { fail(error); } };
el("noAdsBtn").onclick = async () => { if (doc) { doc.boxes = []; render(); try { await save(); } catch (error) { fail(error); } } };
el("discardBtn").onclick = async () => { if (loadedPath) { try { await saveQueue; await lb.discardScreenshot(loadedPath); await refresh(); } catch (error) { fail(error); } } };
document.onkeydown = event => {
  if (event.target instanceof HTMLSelectElement) return;
  if (event.key === "Delete" || event.key === "Backspace") { event.preventDefault(); doc?.boxes.pop(); render(); void save().catch(fail); }
  if (event.key === "ArrowRight") { event.preventDefault(); el("nextBtn").click(); }
  if (event.key === "ArrowLeft") { event.preventDefault(); el("prevBtn").click(); }
  if ((event.ctrlKey || event.metaKey) && event.key === "s") { event.preventDefault(); void save().catch(fail); }
};
void refresh().catch(fail);
