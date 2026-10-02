export function el<T extends HTMLElement = HTMLElement>(id: string): T {
  const element = document.getElementById(id);
  if (!element) throw new Error(`Missing element: ${id}`);
  return element as T;
}
export function errorText(error: unknown): string { const message = error instanceof Error ? error.message : String(error);
  return message.includes("undefined") && message.includes("invoke") ? "Open this interface in the LiveBlock desktop app." : message; }
export function point(event: PointerEvent, stage: HTMLElement): [number, number] {
  const bounds = stage.getBoundingClientRect();
  return [Math.max(0, Math.min(1, (event.clientX - bounds.left) / bounds.width)),
    Math.max(0, Math.min(1, (event.clientY - bounds.top) / bounds.height))];
}
export function rectangle(a: [number, number], b: [number, number]) {
  return { x: Math.min(a[0], b[0]), y: Math.min(a[1], b[1]), width: Math.abs(a[0] - b[0]), height: Math.abs(a[1] - b[1]) };
}
export function positionBox(box: HTMLElement, rect: { x: number; y: number; width: number; height: number }) {
  Object.assign(box.style, { left: `${rect.x * 100}%`, top: `${rect.y * 100}%`, width: `${rect.width * 100}%`, height: `${rect.height * 100}%` });
}
