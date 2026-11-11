/** Tekto controls showcase. Run with packages/wasm/dev.js or tekto dev --js. */
import { createSignal } from "@tektogui/core";
import { appBar, appBarAction } from "@tektogui/jsx-runtime";

// Two colors and a transparent circle make contain/cover and alpha easy to see.
function makeImage() {
  const width = 64, height = 40;
  const rgba = [];
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const inside = (x - 31.5) ** 2 + (y - 19.5) ** 2 < 11 ** 2;
      rgba.push(...(inside ? [244, 243, 240, 80] : x < width / 2 ? [49, 110, 116, 255] : [218, 151, 97, 255]));
    }
  }
  return { width, height, rgba };
}

export function App() {
  const [name, setName] = createSignal("");
  const [count, setCount] = createSignal(0);
  const picture = makeImage();
  return (
    <window title="Tekto showcase" width={640} height={400} fill="#f4f3f0" padding={18} gap={10}>
      {appBar("Tekto showcase", [
        appBarAction("Reset", () => setCount(0)),
        appBarAction("Add ten", () => setCount(value => value + 10)),
      ])}
      <group column={false} gap={14} height={100}>
        <group width={178} gap={5}>
          <text fontSize={14}>Image: contain</text>
          <image name="Teal and orange image, contained" width={166} height={72} imageFit="contain" imageSource={picture} />
        </group>
        <group width={178} gap={5}>
          <text fontSize={14}>Image: cover</text>
          <image name="Teal and orange image, cropped" width={166} height={72} imageFit="cover" imageSource={picture} />
        </group>
        <group width={200} gap={6}>
          <text fontSize={14}>Type, select, copy and paste</text>
          <text_field name="Your name" width={190} height={38} onInput={setName} />
          <text fontSize={14} text={() => name() ? `Hello, ${name()}` : "Hello there"} />
        </group>
      </group>
      <group column={false} gap={14} height={130}>
        <group width={178} gap={8}>
          <text fontSize={14}>Settings</text>
          <switch name="Notifications" checked={true}>Notifications</switch>
          <checkbox name="Remember" checked={false}>Remember</checkbox>
        </group>
        <group width={178} gap={8}>
          <text fontSize={14}>Choose an option</text>
          <select name="Size" width={166} height={36}>
            <menu_item checked={true}>Small</menu_item>
            <menu_item>Medium</menu_item>
            <menu_item>Large</menu_item>
          </select>
          <slider name="Volume" value={0.5} width={166} height={24} />
        </group>
        <group width={200} gap={8}>
          <text fontSize={14}>A reactive button</text>
          <button name="Count clicks" width={160} height={38} onClick={() => setCount(value => value + 1)}>Count clicks</button>
          <text fontSize={14} text={() => `Clicks: ${count()}`} />
        </group>
      </group>
      <progress name="Example progress" width={570} height={12} value={0.65} />
    </window>
  );
}
