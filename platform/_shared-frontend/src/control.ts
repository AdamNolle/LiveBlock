import { logo, toggle } from "./primitives";
import { lb, events } from "./ipc";
import { el, errorText } from "./dom";
let running = false;
let count = 0;
let powerPending = false;
function clearError() {
  el("errorNotice").hidden = true;
  el("errorMessage").textContent = "";
}
function showError(error: unknown) {
  el("errorMessage").textContent = errorText(error);
  el("errorNotice").hidden = false;
}
el("dismissError").onclick = clearError;
const mode = el("blockingMode") as HTMLSelectElement;
mode.value = localStorage.getItem("blockingMode") === "cover" ? "cover" : "capture";
mode.onchange = () => { void syncMode(); };
const power = document.createElement("button");
power.className = "lb-pill lb-block";
power.textContent = "Start";
power.disabled = true;
power.onclick = async () => {
  clearError();
  powerPending = true;
  power.disabled = true;
  mode.disabled = true;
  const on = !running;
  power.textContent = on ? "Starting…" : "Stopping…";
  try { await (on ? mode.value === "cover" ? lb.startPaintOver() : lb.startCapture() : lb.stopCapture()); await refresh(); }
  catch (error) { showError(error); }
  finally { powerPending = false; power.disabled = false; mode.disabled = running; status(); }
};
const detection = toggle({ value: false, accent: "detect", onChange: async on => {
  clearError();
  try { await lb.setDetectionEnabled(on); }
  catch (error) { detection.dataset.on = String(!on); showError(error); }
} });
el("logo").appendChild(logo({ size: 32, radius: 8 }));
el("powerToggle").appendChild(power);
el("appToggle").appendChild(detection);
power.title = "Start or stop blocking (Ctrl+Shift+L)";
detection.setAttribute("aria-label", "Automatic ad detection");
function status() {
  if (!powerPending) power.textContent = running ? "Stop" : "Start";
  el("stateDot").className = running ? "lb-dot success" : "lb-dot";
  const text = `${running ? mode.value === "cover" ? "Cover only" : "Running" : "Stopped"} · ${count} ${count === 1 ? "region" : "regions"}`;
  el("status").textContent = text;
  el("status").title = text;
}
async function act(action: () => Promise<unknown>) {
  clearError();
  try { await action(); } catch (error) { showError(error); }
}
el("blockBtn").onclick = () => { void act(() => lb.showWindow("editor")); };
el("trainBtn").onclick = () => { void act(() => lb.showWindow("training")); };
el("prefsBtn").onclick = () => { void act(() => lb.showWindow("labeling")); };
el("captureBtn").onclick = () => { void act(async () => {
  const path = await lb.captureScreenshotForLabeling();
  if (!path) throw new Error("Start capture before taking a screenshot.");
  await lb.showWindow("labeling");
}); };
events.onCaptureState(on => { running = on; power.dataset.on = String(on); power.setAttribute("aria-pressed", String(on)); status(); }).catch(showError);
events.onOperationFailed(showError).catch(showError);
async function refresh() {
  try { const state = await lb.captureStatus(); running = state.running; if (running) mode.value = state.coverOnly ? "cover" : "capture"; mode.disabled = running || powerPending; const coverOnly = mode.value === "cover"; power.dataset.on = String(running); power.setAttribute("aria-pressed", String(running)); detection.disabled = coverOnly || !state.detectorReady; detection.title = coverOnly ? "Automatic detection needs screen capture" : state.detectorReady ? "Automatically cover detected ads" : "Install a trained model and restart capture"; (el("captureBtn") as HTMLButtonElement).disabled = coverOnly; el("overlayNotice").hidden = state.overlayMode !== "FloatingDegraded"; detection.dataset.on = String(state.detectionEnabled); detection.setAttribute("aria-checked", String(state.detectionEnabled)); count = (await lb.listRegions()).length; el("statRegions").textContent = String(count); status(); }
  catch (error) { showError(error); }
}
async function syncMode() {
  clearError();
  powerPending = true;
  power.disabled = true;
  mode.disabled = true;
  try {
    await lb.setCoverOnlyPreferred(mode.value === "cover");
    localStorage.setItem("blockingMode", mode.value);
  } catch (error) { showError(error); }
  finally {
    powerPending = false;
    power.disabled = false;
    await refresh();
  }
}
async function initialize() {
  await refresh();
  if (!running) await syncMode();
  else power.disabled = false;
}
void initialize();
setInterval(refresh, 1000);
