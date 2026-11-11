/** Desktop QuickJS example. See docs/tsx-events.md for run and inspect steps. */
import { createSignal } from "@tektogui/core";

export function App() {
  const [count, setCount] = createSignal(0);
  return (
    <window title="Counter" width={640} height={400} fill="#f4f3f0" padding={24} gap={16}>
      <text fontSize={22} color="#2c333a">Counter</text>
      <text name="Count" fontSize={20} color="#2c333a" text={() => count()} />
      <button name="Increment" fill="#dce3df" color="#2c333a" fontSize={18} padding={12}
        onClick={() => setCount(n => n + 1)}>Increment</button>
    </window>
  );
}
