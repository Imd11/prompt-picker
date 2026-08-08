import { readFileSync } from "node:fs";
import { inflateSync } from "node:zlib";

type Presentation = {
  scale: number;
  offsetX: number;
  offsetY: number;
};

export type SheetGeometry = {
  file: string;
  frameWidth: number;
  frameHeight: number;
  frameCount: number;
  columns: number;
  strideX: number;
  strideY: number;
};

type DecodedPng = {
  width: number;
  height: number;
  pixels: Uint8Array;
};

export type MotionVisualMetrics = {
  medianPrimaryArea: number;
  minimumHitCoverage: number;
  nativeWindowContained: boolean;
};

const PNG_SIGNATURE = "89504e470d0a1a0a";
const ALPHA_THRESHOLD = 32;
const SPRITE_SIZE = 126;
const HIT_AREA_SIZE = 132;
const NATIVE_WINDOW_SIZE = 288;
const CANVAS_INSET = (HIT_AREA_SIZE - SPRITE_SIZE) / 2;
const NATIVE_VISUAL_INSET = (NATIVE_WINDOW_SIZE - HIT_AREA_SIZE) / 2;
const TRANSFORM_ORIGIN_X = SPRITE_SIZE * 0.45;
const TRANSFORM_ORIGIN_Y = SPRITE_SIZE * 0.76;

function paeth(left: number, above: number, upperLeft: number) {
  const estimate = left + above - upperLeft;
  const leftDistance = Math.abs(estimate - left);
  const aboveDistance = Math.abs(estimate - above);
  const upperLeftDistance = Math.abs(estimate - upperLeft);
  if (leftDistance <= aboveDistance && leftDistance <= upperLeftDistance) return left;
  if (aboveDistance <= upperLeftDistance) return above;
  return upperLeft;
}

export function decodeRgbaPng(path: string): DecodedPng {
  const png = readFileSync(path);
  if (png.subarray(0, 8).toString("hex") !== PNG_SIGNATURE) {
    throw new Error(`Invalid PNG signature: ${path}`);
  }

  let width = 0;
  let height = 0;
  let bitDepth = 0;
  let colorType = 0;
  let interlace = 0;
  const compressed: Buffer[] = [];
  for (let offset = 8; offset + 12 <= png.length;) {
    const length = png.readUInt32BE(offset);
    const type = png.toString("ascii", offset + 4, offset + 8);
    const dataStart = offset + 8;
    if (type === "IHDR") {
      width = png.readUInt32BE(dataStart);
      height = png.readUInt32BE(dataStart + 4);
      bitDepth = png[dataStart + 8];
      colorType = png[dataStart + 9];
      interlace = png[dataStart + 12];
    } else if (type === "IDAT") {
      compressed.push(png.subarray(dataStart, dataStart + length));
    } else if (type === "IEND") {
      break;
    }
    offset += length + 12;
  }
  if (!width || !height || bitDepth !== 8 || colorType !== 6 || interlace !== 0) {
    throw new Error(`Expected a non-interlaced 8-bit RGBA PNG: ${path}`);
  }

  const source = inflateSync(Buffer.concat(compressed));
  const rowLength = width * 4;
  const pixels = new Uint8Array(rowLength * height);
  let sourceOffset = 0;
  for (let y = 0; y < height; y += 1) {
    const filter = source[sourceOffset];
    sourceOffset += 1;
    const rowOffset = y * rowLength;
    for (let x = 0; x < rowLength; x += 1) {
      const raw = source[sourceOffset + x];
      const left = x >= 4 ? pixels[rowOffset + x - 4] : 0;
      const above = y > 0 ? pixels[rowOffset - rowLength + x] : 0;
      const upperLeft = y > 0 && x >= 4 ? pixels[rowOffset - rowLength + x - 4] : 0;
      let predictor = 0;
      if (filter === 1) predictor = left;
      else if (filter === 2) predictor = above;
      else if (filter === 3) predictor = Math.floor((left + above) / 2);
      else if (filter === 4) predictor = paeth(left, above, upperLeft);
      else if (filter !== 0) throw new Error(`Unsupported PNG filter ${filter}: ${path}`);
      pixels[rowOffset + x] = (raw + predictor) & 0xff;
    }
    sourceOffset += rowLength;
  }
  return { width, height, pixels };
}

function primaryComponent(mask: Uint8Array, width: number, height: number) {
  const visited = new Uint8Array(mask.length);
  const stack = new Int32Array(mask.length);
  let largest: number[] = [];
  for (let start = 0; start < mask.length; start += 1) {
    if (!mask[start] || visited[start]) continue;
    let stackLength = 1;
    stack[0] = start;
    visited[start] = 1;
    const component: number[] = [];
    while (stackLength > 0) {
      const index = stack[--stackLength];
      component.push(index);
      const x = index % width;
      const y = Math.floor(index / width);
      for (let dy = -1; dy <= 1; dy += 1) {
        for (let dx = -1; dx <= 1; dx += 1) {
          if (dx === 0 && dy === 0) continue;
          const nextX = x + dx;
          const nextY = y + dy;
          if (nextX < 0 || nextX >= width || nextY < 0 || nextY >= height) continue;
          const next = nextY * width + nextX;
          if (!mask[next] || visited[next]) continue;
          visited[next] = 1;
          stack[stackLength] = next;
          stackLength += 1;
        }
      }
    }
    if (component.length > largest.length) largest = component;
  }
  return largest;
}

function median(values: number[]) {
  const sorted = [...values].sort((left, right) => left - right);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0
    ? (sorted[middle - 1] + sorted[middle]) / 2
    : sorted[middle];
}

export function measureMotionVisual(
  png: DecodedPng,
  sheet: SheetGeometry,
  presentation: Presentation,
  rotationDegrees = 0,
  sourceRegionBottom = sheet.frameHeight,
): MotionVisualMetrics {
  if (png.width < sheet.frameWidth || png.height < sheet.frameHeight) {
    throw new Error("Sprite sheet is smaller than one frame");
  }
  const containScale = Math.min(
    SPRITE_SIZE / sheet.frameWidth,
    SPRITE_SIZE / sheet.frameHeight,
  );
  const destinationX = (SPRITE_SIZE - sheet.frameWidth * containScale) / 2;
  const destinationY = (SPRITE_SIZE - sheet.frameHeight * containScale) / 2;
  const radians = rotationDegrees * Math.PI / 180;
  const cosine = Math.cos(radians);
  const sine = Math.sin(radians);
  const areas: number[] = [];
  let minimumHitCoverage = 1;
  let nativeWindowContained = true;

  for (let frame = 0; frame < sheet.frameCount; frame += 1) {
    const column = frame % sheet.columns;
    const row = Math.floor(frame / sheet.columns);
    const sourceX = column * sheet.strideX;
    const sourceY = row * sheet.strideY;
    const mask = new Uint8Array(sheet.frameWidth * sheet.frameHeight);
    for (let y = 0; y < sheet.frameHeight; y += 1) {
      for (let x = 0; x < sheet.frameWidth; x += 1) {
        const alphaIndex = ((sourceY + y) * png.width + sourceX + x) * 4 + 3;
        if (y < sourceRegionBottom && png.pixels[alphaIndex] >= ALPHA_THRESHOLD) {
          mask[y * sheet.frameWidth + x] = 1;
        }
      }
    }
    const component = primaryComponent(mask, sheet.frameWidth, sheet.frameHeight);
    if (component.length === 0) throw new Error(`Frame ${frame} has no visible component`);
    areas.push(component.length * (containScale * presentation.scale) ** 2);

    let hitPixels = 0;
    for (const index of component) {
      const sourcePixelX = index % sheet.frameWidth;
      const sourcePixelY = Math.floor(index / sheet.frameWidth);
      const containedX = destinationX + (sourcePixelX + 0.5) * containScale;
      const containedY = destinationY + (sourcePixelY + 0.5) * containScale;
      const scaledX = TRANSFORM_ORIGIN_X
        + (containedX - TRANSFORM_ORIGIN_X) * presentation.scale;
      const scaledY = TRANSFORM_ORIGIN_Y
        + (containedY - TRANSFORM_ORIGIN_Y) * presentation.scale;
      const relativeX = scaledX - TRANSFORM_ORIGIN_X;
      const relativeY = scaledY - TRANSFORM_ORIGIN_Y;
      const rotatedX = TRANSFORM_ORIGIN_X + relativeX * cosine - relativeY * sine;
      const rotatedY = TRANSFORM_ORIGIN_Y + relativeX * sine + relativeY * cosine;
      const hitX = CANVAS_INSET + presentation.offsetX + rotatedX;
      const hitY = CANVAS_INSET + presentation.offsetY + rotatedY;
      if (hitX >= 0 && hitX <= HIT_AREA_SIZE && hitY >= 0 && hitY <= HIT_AREA_SIZE) {
        hitPixels += 1;
      }
      const nativeX = NATIVE_VISUAL_INSET + hitX;
      const nativeY = NATIVE_VISUAL_INSET + hitY;
      if (nativeX < 0 || nativeX > NATIVE_WINDOW_SIZE
          || nativeY < 0 || nativeY > NATIVE_WINDOW_SIZE) {
        nativeWindowContained = false;
      }
    }
    minimumHitCoverage = Math.min(minimumHitCoverage, hitPixels / component.length);
  }

  return {
    medianPrimaryArea: median(areas),
    minimumHitCoverage,
    nativeWindowContained,
  };
}
