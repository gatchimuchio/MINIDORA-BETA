"use strict";

class MinidoraMicrophone extends AudioWorkletProcessor {
  constructor({ processorOptions } = {}) {
    super();
    const maxFrames = processorOptions?.maxFrames;
    if (!Number.isSafeInteger(maxFrames) || maxFrames < 1 || maxFrames > 160000) {
      throw new RangeError("invalid microphone recording frame limit");
    }
    this.samples = new Float32Array(maxFrames);
    this.length = 0;
    this.port.onmessage = ({ data }) => {
      if (data === "stop") this.finish(false);
    };
  }

  finish(full) {
    if (this.samples === null) return;
    const samples = this.samples.subarray(0, this.length);
    this.samples = null;
    this.port.postMessage({ samples, full }, [samples.buffer]);
  }

  process(inputs, outputs) {
    for (const channels of outputs) {
      for (const channel of channels) channel.fill(0);
    }
    if (this.samples === null) return false;
    const mono = inputs[0]?.[0];
    if (!mono) return true;

    const count = Math.min(mono.length, this.samples.length - this.length);
    this.samples.set(mono.subarray(0, count), this.length);
    this.length += count;
    if (this.length < this.samples.length) return true;
    this.finish(true);
    return false;
  }
}

registerProcessor("minidora-microphone", MinidoraMicrophone);
