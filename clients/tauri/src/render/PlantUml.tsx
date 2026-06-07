import { useState } from "react";
import pako from "pako";
import { Fallback } from "./Fallback";

// PlantUML's text encoding: deflateRaw, then its own base64 variant.
function encode6bit(b: number): string {
  if (b < 10) return String.fromCharCode(48 + b);
  b -= 10;
  if (b < 26) return String.fromCharCode(65 + b);
  b -= 26;
  if (b < 26) return String.fromCharCode(97 + b);
  b -= 26;
  if (b === 0) return "-";
  if (b === 1) return "_";
  return "?";
}
function append3(b1: number, b2: number, b3: number): string {
  const c1 = b1 >> 2;
  const c2 = ((b1 & 0x3) << 4) | (b2 >> 4);
  const c3 = ((b2 & 0xf) << 2) | (b3 >> 6);
  const c4 = b3 & 0x3f;
  return encode6bit(c1 & 0x3f) + encode6bit(c2 & 0x3f) + encode6bit(c3 & 0x3f) + encode6bit(c4 & 0x3f);
}
function encode64(data: Uint8Array): string {
  let r = "";
  for (let i = 0; i < data.length; i += 3) {
    if (i + 2 === data.length) r += append3(data[i], data[i + 1], 0);
    else if (i + 1 === data.length) r += append3(data[i], 0, 0);
    else r += append3(data[i], data[i + 1], data[i + 2]);
  }
  return r;
}
function plantumlEncode(text: string): string {
  const utf8 = new TextEncoder().encode(text);
  const deflated = pako.deflateRaw(utf8, { level: 9 });
  return encode64(deflated);
}

/** Render a ```plantuml fenced block via a PlantUML render server (SVG image). */
export function PlantUml({ code, server }: { code: string; server: string }) {
  const [failed, setFailed] = useState(false);
  let src: string;
  try {
    src = `${server.replace(/\/+$/, "")}/svg/${plantumlEncode(code)}`;
  } catch {
    return <Fallback title="Couldn't encode this PlantUML diagram" source={code} />;
  }
  if (failed) {
    return <Fallback title="Couldn't load this PlantUML diagram (render server unreachable?)" source={code} />;
  }
  return (
    <div className="my-3 flex justify-center overflow-x-auto">
      <img
        src={src}
        alt="PlantUML diagram"
        onError={() => setFailed(true)}
        className="max-w-full rounded bg-white p-2"
      />
    </div>
  );
}
