import type { JSX, HostApi } from "./src/index.js";

const field: JSX.IntrinsicElements["text_field"] = { onInput: value => { const text: string = value; void text; } };
const host: Pick<HostApi, "onInput"> = { onInput: (_id, callback) => callback("value") };
// @ts-expect-error input values are strings
const wrong: JSX.IntrinsicElements["text_field"] = { onInput: (value: number) => {} };
// @ts-expect-error buttons do not support onInput
const button: JSX.IntrinsicElements["button"] = { onInput: () => {} };
void [field, host, wrong, button];
