export const RANDOM_SEED_BYTES = 32;
export const RANDOM_SEED_HEX_LEN = 64;
export const DERIVE_SEED_DOMAIN_PREFIX = "randseed-chacha20-derive-v1\0";

const MAX_SAFE_BIGINT = BigInt(Number.MAX_SAFE_INTEGER);
const MASK_32 = 0xffff_ffffn;
const TWO_POW_32 = 0x1_0000_0000n;
const UINT32_SCALE = 4294967296;
const SEED_HEX_REGEX = /^[0-9a-f]{64}$/;
const UTF8_ENCODER = new TextEncoder();

const CHACHA20_CONSTANTS = new Uint32Array([
  0x61707865,
  0x3320646e,
  0x79622d32,
  0x6b206574,
]);

const SHA256_INIT = new Uint32Array([
  0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
  0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
]);

const SHA256_K = new Uint32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
]);

export interface ChaCha20State {
  seedHex: string;
  stream: number;
  wordPos: number;
}

export function isValidSeedHex(value: unknown): value is string {
  return typeof value === "string" && SEED_HEX_REGEX.test(value);
}

export function parseSeedHex(seedHex: string): Uint8Array {
  if (!isValidSeedHex(seedHex)) {
    throw new Error("randomSeed must be a 64-character lowercase hex string.");
  }
  const bytes = new Uint8Array(RANDOM_SEED_BYTES);
  for (let index = 0; index < RANDOM_SEED_BYTES; index += 1) {
    bytes[index] = Number.parseInt(seedHex.slice(index * 2, index * 2 + 2), 16);
  }
  return bytes;
}

export function encodeSeedHex(seed: Uint8Array): string {
  if (!(seed instanceof Uint8Array) || seed.length !== RANDOM_SEED_BYTES) {
    throw new Error("Seed must be a 32-byte Uint8Array.");
  }
  let hex = "";
  for (let index = 0; index < RANDOM_SEED_BYTES; index += 1) {
    hex += seed[index]!.toString(16).padStart(2, "0");
  }
  return hex;
}

export function legacySeedFromBytes(seed: Uint8Array | string): number {
  const bytes = typeof seed === "string" ? parseSeedHex(seed) : seed;
  if (!(bytes instanceof Uint8Array) || bytes.length !== RANDOM_SEED_BYTES) {
    throw new Error("Seed must be 32 bytes.");
  }
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  return view.getUint32(0, true);
}

function normalizeSeedBytes(seed: Uint8Array | string): Uint8Array {
  if (typeof seed === "string") {
    return parseSeedHex(seed);
  }
  if (!(seed instanceof Uint8Array) || seed.length !== RANDOM_SEED_BYTES) {
    throw new Error("Seed must be a 32-byte Uint8Array or 64-character lowercase hex string.");
  }
  return new Uint8Array(seed);
}

function normalizeU64(value: number | bigint, label: string): bigint {
  if (typeof value === "bigint") {
    if (value < 0n || value > 0xffff_ffff_ffff_ffffn) {
      throw new Error(`${label} must be a non-negative 64-bit integer.`);
    }
    return value;
  }
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error(`${label} must be a non-negative safe integer.`);
  }
  return BigInt(value);
}

function rotl32(value: number, shift: number): number {
  return ((value << shift) | (value >>> (32 - shift))) >>> 0;
}

function rotr32(value: number, shift: number): number {
  return ((value >>> shift) | (value << (32 - shift))) >>> 0;
}

function quarterRound(state: Uint32Array, a: number, b: number, c: number, d: number): void {
  state[a] = (state[a]! + state[b]!) >>> 0;
  state[d] = rotl32(state[d]! ^ state[a]!, 16);

  state[c] = (state[c]! + state[d]!) >>> 0;
  state[b] = rotl32(state[b]! ^ state[c]!, 12);

  state[a] = (state[a]! + state[b]!) >>> 0;
  state[d] = rotl32(state[d]! ^ state[a]!, 8);

  state[c] = (state[c]! + state[d]!) >>> 0;
  state[b] = rotl32(state[b]! ^ state[c]!, 7);
}

function chacha20Block(keyWords: Uint32Array, blockCounter: bigint, stream: bigint): Uint32Array {
  const initial = new Uint32Array(16);
  initial.set(CHACHA20_CONSTANTS, 0);
  initial.set(keyWords, 4);
  initial[12] = Number(blockCounter & MASK_32);
  initial[13] = Number((blockCounter >> 32n) & MASK_32);
  initial[14] = Number(stream & MASK_32);
  initial[15] = Number((stream >> 32n) & MASK_32);

  const working = new Uint32Array(initial);
  for (let round = 0; round < 10; round += 1) {
    quarterRound(working, 0, 4, 8, 12);
    quarterRound(working, 1, 5, 9, 13);
    quarterRound(working, 2, 6, 10, 14);
    quarterRound(working, 3, 7, 11, 15);

    quarterRound(working, 0, 5, 10, 15);
    quarterRound(working, 1, 6, 11, 12);
    quarterRound(working, 2, 7, 8, 13);
    quarterRound(working, 3, 4, 9, 14);
  }

  for (let index = 0; index < 16; index += 1) {
    working[index] = (working[index]! + initial[index]!) >>> 0;
  }
  return working;
}

function sha256(chunks: Uint8Array[]): Uint8Array {
  let totalBytes = 0;
  for (const chunk of chunks) {
    totalBytes += chunk.length;
  }

  const paddedLen = Math.ceil((totalBytes + 9) / 64) * 64;
  const message = new Uint8Array(paddedLen);
  let offset = 0;
  for (const chunk of chunks) {
    message.set(chunk, offset);
    offset += chunk.length;
  }
  message[offset] = 0x80;

  const totalBits = BigInt(totalBytes) * 8n;
  const msgView = new DataView(message.buffer);
  msgView.setUint32(paddedLen - 8, Number((totalBits >> 32n) & MASK_32), false);
  msgView.setUint32(paddedLen - 4, Number(totalBits & MASK_32), false);

  const state = new Uint32Array(SHA256_INIT);
  const w = new Uint32Array(64);

  for (let blockStart = 0; blockStart < paddedLen; blockStart += 64) {
    for (let i = 0; i < 16; i += 1) {
      w[i] = msgView.getUint32(blockStart + i * 4, false);
    }
    for (let i = 16; i < 64; i += 1) {
      const w15 = w[i - 15]!;
      const w2 = w[i - 2]!;
      const s0 = (rotr32(w15, 7) ^ rotr32(w15, 18) ^ (w15 >>> 3)) >>> 0;
      const s1 = (rotr32(w2, 17) ^ rotr32(w2, 19) ^ (w2 >>> 10)) >>> 0;
      w[i] = (w[i - 16]! + s0 + w[i - 7]! + s1) >>> 0;
    }

    let a = state[0]!;
    let b = state[1]!;
    let c = state[2]!;
    let d = state[3]!;
    let e = state[4]!;
    let f = state[5]!;
    let g = state[6]!;
    let h = state[7]!;

    for (let i = 0; i < 64; i += 1) {
      const s1 = (rotr32(e, 6) ^ rotr32(e, 11) ^ rotr32(e, 25)) >>> 0;
      const ch = ((e & f) ^ ((~e) & g)) >>> 0;
      const temp1 = (h + s1 + ch + SHA256_K[i]! + w[i]!) >>> 0;
      const s0 = (rotr32(a, 2) ^ rotr32(a, 13) ^ rotr32(a, 22)) >>> 0;
      const maj = ((a & b) ^ (a & c) ^ (b & c)) >>> 0;
      const temp2 = (s0 + maj) >>> 0;

      h = g;
      g = f;
      f = e;
      e = (d + temp1) >>> 0;
      d = c;
      c = b;
      b = a;
      a = (temp1 + temp2) >>> 0;
    }

    state[0] = (state[0]! + a) >>> 0;
    state[1] = (state[1]! + b) >>> 0;
    state[2] = (state[2]! + c) >>> 0;
    state[3] = (state[3]! + d) >>> 0;
    state[4] = (state[4]! + e) >>> 0;
    state[5] = (state[5]! + f) >>> 0;
    state[6] = (state[6]! + g) >>> 0;
    state[7] = (state[7]! + h) >>> 0;
  }

  const digest = new Uint8Array(32);
  const digestView = new DataView(digest.buffer);
  for (let i = 0; i < 8; i += 1) {
    digestView.setUint32(i * 4, state[i]!, false);
  }
  return digest;
}

function u32LeBytes(value: number): Uint8Array {
  const buf = new Uint8Array(4);
  new DataView(buf.buffer).setUint32(0, value >>> 0, true);
  return buf;
}

/**
 * Derives a collision-free 32-byte sub-seed from `rootSeed` and length-prefixed `domains` via SHA-256.
 */
export function deriveSeed(
  rootSeed: Uint8Array | string,
  ...domains: Array<string | Uint8Array>
): Uint8Array {
  const root = normalizeSeedBytes(rootSeed);
  const chunks: Uint8Array[] = [
    UTF8_ENCODER.encode(DERIVE_SEED_DOMAIN_PREFIX),
    root,
    u32LeBytes(domains.length),
  ];
  for (const domain of domains) {
    const bytes = typeof domain === "string" ? UTF8_ENCODER.encode(domain) : domain;
    if (!(bytes instanceof Uint8Array)) {
      throw new Error("Each domain must be a string or Uint8Array.");
    }
    chunks.push(u32LeBytes(bytes.length), bytes);
  }
  return sha256(chunks);
}

/**
 * Derives a collision-free 64-character lowercase hex sub-seed.
 */
export function deriveSeedHex(
  rootSeed: Uint8Array | string,
  ...domains: Array<string | Uint8Array>
): string {
  return encodeSeedHex(deriveSeed(rootSeed, ...domains));
}

/**
 * Deterministic 20-round ChaCha20 PRNG matching Rust `rand_chacha::ChaCha20Rng`.
 */
export class ChaCha20Rng {
  private readonly seedBytes: Uint8Array;
  private readonly keyWords: Uint32Array;
  private streamId: bigint;
  private wordPosition: bigint;
  private cachedBlockIndex: bigint | null = null;
  private cachedBlock: Uint32Array | null = null;

  constructor(
    seed: Uint8Array | string,
    options?: { stream?: number | bigint; wordPos?: number | bigint },
  ) {
    this.seedBytes = normalizeSeedBytes(seed);
    this.keyWords = new Uint32Array(8);
    const view = new DataView(
      this.seedBytes.buffer,
      this.seedBytes.byteOffset,
      this.seedBytes.byteLength,
    );
    for (let index = 0; index < 8; index += 1) {
      this.keyWords[index] = view.getUint32(index * 4, true);
    }
    this.streamId = normalizeU64(options?.stream ?? 0, "stream");
    this.wordPosition = normalizeU64(options?.wordPos ?? 0, "wordPos");
  }

  static fromDerived(
    rootSeed: Uint8Array | string,
    ...domains: Array<string | Uint8Array>
  ): ChaCha20Rng {
    return new ChaCha20Rng(deriveSeed(rootSeed, ...domains));
  }

  static fromState(state: ChaCha20State): ChaCha20Rng {
    if (
      typeof state !== "object" ||
      state === null ||
      !isValidSeedHex(state.seedHex) ||
      !Number.isSafeInteger(state.stream) ||
      state.stream < 0 ||
      !Number.isSafeInteger(state.wordPos) ||
      state.wordPos < 0
    ) {
      throw new Error("Invalid ChaCha20State.");
    }
    return new ChaCha20Rng(state.seedHex, {
      stream: state.stream,
      wordPos: state.wordPos,
    });
  }

  getState(): ChaCha20State {
    if (this.streamId > MAX_SAFE_BIGINT || this.wordPosition > MAX_SAFE_BIGINT) {
      throw new Error("ChaCha20Rng state exceeds JSON safe integer bounds.");
    }
    return {
      seedHex: encodeSeedHex(this.seedBytes),
      stream: Number(this.streamId),
      wordPos: Number(this.wordPosition),
    };
  }

  get seedHex(): string {
    return encodeSeedHex(this.seedBytes);
  }

  get stream(): number {
    return Number(this.streamId);
  }

  get wordPos(): number {
    return Number(this.wordPosition);
  }

  setStream(stream: number | bigint): void {
    const next = normalizeU64(stream, "stream");
    if (this.streamId !== next) {
      this.streamId = next;
      this.cachedBlockIndex = null;
      this.cachedBlock = null;
    }
  }

  setWordPos(wordPos: number | bigint): void {
    this.wordPosition = normalizeU64(wordPos, "wordPos");
  }

  nextU32(): number {
    const blockIndex = this.wordPosition >> 4n;
    const wordOffset = Number(this.wordPosition & 15n);
    if (this.cachedBlockIndex !== blockIndex || this.cachedBlock === null) {
      this.cachedBlock = chacha20Block(this.keyWords, blockIndex, this.streamId);
      this.cachedBlockIndex = blockIndex;
    }
    this.wordPosition += 1n;
    return this.cachedBlock[wordOffset]!;
  }

  nextU64(): bigint {
    const lo = BigInt(this.nextU32());
    const hi = BigInt(this.nextU32());
    return lo | (hi << 32n);
  }

  nextFloat(): number {
    return this.nextU32() / UINT32_SCALE;
  }

  nextBounded(bound: number): number {
    if (!Number.isInteger(bound) || bound <= 0 || bound > 0xffff_ffff) {
      throw new Error("bound must be an integer in 1..=4294967295.");
    }
    const boundBig = BigInt(bound);
    const limit = TWO_POW_32 - (TWO_POW_32 % boundBig);
    while (true) {
      const candidate = BigInt(this.nextU32());
      if (candidate < limit) {
        return Number(candidate % boundBig);
      }
    }
  }

  nextIntRange(min: number, maxInclusive: number): number {
    if (
      !Number.isInteger(min) ||
      !Number.isInteger(maxInclusive) ||
      min < -0x8000_0000 ||
      maxInclusive > 0x7fff_ffff ||
      min > maxInclusive
    ) {
      throw new Error("min and maxInclusive must be signed 32-bit integers with min <= maxInclusive.");
    }
    const span = maxInclusive - min;
    if (span === 0xffff_ffff) {
      return min + this.nextU32();
    }
    return min + this.nextBounded(span + 1);
  }

  nextBool(): boolean {
    return this.nextBounded(2) === 1;
  }

  shuffle<T>(items: T[]): T[] {
    for (let i = items.length - 1; i >= 1; i -= 1) {
      const j = this.nextBounded(i + 1);
      const tmp = items[i]!;
      items[i] = items[j]!;
      items[j] = tmp;
    }
    return items;
  }
}

/**
 * Creates a deterministic Mulberry32 generator that returns values in [0, 1).
 * Retained for legacy compatibility with `options.seed`.
 */
export function mulberry32(seed: number): () => number {
  let a = seed | 0;
  return function next(): number {
    a |= 0;
    a = (a + 0x6D2B79F5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/**
 * Creates a deterministic Mulberry32 generator that returns unsigned 32-bit integers.
 * Retained for legacy compatibility with `options.seed`.
 */
export function mulberry32U32(seed: number): () => number {
  let a = seed | 0;
  return function nextU32(): number {
    a |= 0;
    a = (a + 0x6D2B79F5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return (t ^ (t >>> 14)) >>> 0;
  };
}
