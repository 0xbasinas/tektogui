/** Static app-supplied pixels; no browser or network APIs are needed. */
export function ImageSketch() {
  const logo = { width: 2, height: 1, rgba: [255, 0, 0, 255, 0, 0, 255, 255] };
  return <window name="Images"><image name="Red and blue logo" width={120} height={80} imageFit="contain" imageSource={logo} /></window>;
}
