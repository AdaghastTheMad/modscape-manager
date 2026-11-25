// The optional button click (Settings → Interface; off by default). Made in
// code with Web Audio, so there's no sound file and nothing borrowed from the game.
import { getPrefs } from "./prefs";

let ctx: AudioContext | null = null;

/** Click if the setting is on: "button" for buttons, tabs and their shortcuts,
 *  "list" for picking an item in a list (its own setting). */
export function uiClick(what: "button" | "list" = "button") {
  const p = getPrefs();
  if (p.clickSound && (what === "button" || p.clickSoundLists)) playClick();
}

export function playClick() {
  try {
    ctx ??= new AudioContext();
    if (ctx.state === "suspended") void ctx.resume();
    const t = ctx.currentTime;
    // A short filtered noise tick plus a low, fast-decaying tone: a soft "tock".
    const len = Math.floor(ctx.sampleRate * 0.03);
    const buf = ctx.createBuffer(1, len, ctx.sampleRate);
    const d = buf.getChannelData(0);
    for (let i = 0; i < len; i++) d[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / len, 4);
    const noise = ctx.createBufferSource();
    noise.buffer = buf;
    const bp = ctx.createBiquadFilter();
    bp.type = "bandpass"; bp.frequency.value = 2400; bp.Q.value = 1.2;
    const ng = ctx.createGain(); ng.gain.value = 0.18;
    noise.connect(bp).connect(ng).connect(ctx.destination);
    const osc = ctx.createOscillator();
    osc.type = "triangle";
    osc.frequency.setValueAtTime(520, t);
    osc.frequency.exponentialRampToValueAtTime(180, t + 0.06);
    const og = ctx.createGain();
    og.gain.setValueAtTime(0.16, t);
    og.gain.exponentialRampToValueAtTime(0.0001, t + 0.07);
    osc.connect(og).connect(ctx.destination);
    noise.start(t); osc.start(t); osc.stop(t + 0.08);
  } catch { /* no audio: stay silent */ }
}
