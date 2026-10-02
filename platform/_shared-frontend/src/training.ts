import { wordmark } from "./primitives";
import { lb, events } from "./ipc";
import { el, errorText } from "./dom";
el("brand").appendChild(wordmark(14));
const train = el<HTMLButtonElement>("trainBtn");
const cancel = el<HTMLButtonElement>("cancelBtn");
function log(line: string) {
  const tail = el("logTail"); tail.textContent = `${tail.textContent}\n${line}`.split("\n").slice(-200).join("\n");
  tail.parentElement!.scrollTop = tail.parentElement!.scrollHeight;
}
function busy(value: boolean) { train.disabled = value; cancel.disabled = !value; }
train.onclick = async () => {
  busy(true); el("trainingState").textContent = "Training";
  try { await lb.startTraining(Number(el<HTMLInputElement>("epochs").value), Number(el<HTMLInputElement>("batch").value), Number(el<HTMLInputElement>("imgsz").value), el<HTMLInputElement>("dataset").value.trim()); }
  catch (error) { log(errorText(error)); busy(false); el("trainingState").textContent = "Failed to start"; }
};
cancel.onclick = async () => {
  try { await lb.cancelTraining(); el("trainingState").textContent = "Cancelling"; }
  catch (error) { log(errorText(error)); }
};
el("labelBtn").onclick = () => { void lb.showWindow("labeling").catch(error => log(errorText(error))); };
events.onTrainingLog(log).catch(error => log(errorText(error)));
events.onTrainingFinished(code => { busy(false); el("trainingState").textContent = code === 0 ? "Complete — restart capture to load the model" : "Training failed or cancelled"; }).catch(error => log(errorText(error)));
async function refresh() {
  try {
    const [shots, regions, status] = await Promise.all([lb.listScreenshots(), lb.listRegions(), lb.captureStatus()]);
    busy(status.trainingRunning);
    const labeled = shots.filter(shot => shot.labeled).length;
    el("statShots").textContent = String(shots.length); el("statLabeled").textContent = String(labeled);
    el("statRem").textContent = String(shots.length - labeled); el("statRegions").textContent = String(regions.length);
  } catch (error) { log(errorText(error)); }
}
void refresh(); setInterval(refresh, 5000);
