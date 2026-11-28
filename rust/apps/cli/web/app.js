"use strict";

const form = document.querySelector("#request-form");
const actorField = document.querySelector("#actor-id");
const caseField = document.querySelector("#case-id");
const tokenField = document.querySelector("#api-token");
const requestField = document.querySelector("#request-text");
const responseLocaleField = document.querySelector("#response-locale");
const webReadButton = document.querySelector("#web-read-hosts-button");
const webReadHosts = document.querySelector("#web-read-hosts");
const webReadStatus = document.querySelector("#web-read-status");
const attachmentField = document.querySelector("#attachment-files");
const attachmentList = document.querySelector("#attachment-list");
const attachmentClearButton = document.querySelector("#attachment-clear-button");
const attachmentStatus = document.querySelector("#attachment-status");
const microphoneStart = document.querySelector("#microphone-start");
const microphoneStop = document.querySelector("#microphone-stop");
const microphoneCancel = document.querySelector("#microphone-cancel");
const microphoneStatus = document.querySelector("#microphone-status");
const speechFileField = document.querySelector("#speech-file");
const speechFileStatus = document.querySelector("#speech-file-status");
const speechClearButton = document.querySelector("#speech-clear-button");
const speechSendButton = document.querySelector("#speech-send-button");
const submitButton = document.querySelector("#submit-button");
const cancelCaseButton = document.querySelector("#cancel-case-button");
const streamEvents = document.querySelector("#stream-events");
const connectionStatus = document.querySelector("#connection-status");
const formError = document.querySelector("#form-error");
const resultState = document.querySelector("#result-state");
const emptyState = document.querySelector("#empty-state");
const loadingState = document.querySelector("#loading-state");
const resultContent = document.querySelector("#result-content");
const resultMeta = document.querySelector("#result-meta");
const recognitionDetails = document.querySelector("#recognition-details");
const recognitionText = document.querySelector("#recognition-text");
const recognitionRange = document.querySelector("#recognition-range");
const answerText = document.querySelector("#answer-text");
const mediaPreview = document.querySelector("#media-preview");
const imagePreview = document.querySelector("#image-preview");
const audioPreview = document.querySelector("#audio-preview");
const mediaPreviewStatus = document.querySelector("#media-preview-status");
const reasonText = document.querySelector("#reason-text");
const attachmentGuidance = document.querySelector("#attachment-guidance");
const downloadAnswerButton = document.querySelector("#download-answer-button");
const downloadAnswerStatus = document.querySelector("#download-answer-status");
const dependencyList = document.querySelector("#dependency-list");
const recordJson = document.querySelector("#record-json");
const openCaseButton = document.querySelector("#open-case-button");
const caseHistoryStatus = document.querySelector("#case-history-status");
const branchAnchor = document.querySelector("#branch-anchor");
const ownHistory = document.querySelector("#own-history");
const inheritedHistory = document.querySelector("#inherited-history");
const forkForm = document.querySelector("#fork-form");
const forkCaseField = document.querySelector("#fork-case-id");
const forkSource = document.querySelector("#fork-source-id");
const forkCurrentButton = document.querySelector("#fork-current-button");
const forkSubmitButton = document.querySelector("#fork-submit-button");
const forkStatus = document.querySelector("#fork-status");
const ownPager = historyPager("own-history");
const inheritedPager = historyPager("inherited-history");

const MAX_INPUT_BYTES = 256000;
const MAX_ATTACHMENTS = 8;
const MAX_BINARY_ATTACHMENT_BYTES = 192000;
const SPEECH_SAMPLE_RATE = 16000;
const MAX_SPEECH_FRAMES = SPEECH_SAMPLE_RATE * 10;
const PCM_HEADER_BYTES = 44;
const MAX_RECOGNITION_BYTES = 2048;
const MAX_MICROPHONE_FRAMES = Math.min(
  MAX_SPEECH_FRAMES,
  Math.floor((MAX_INPUT_BYTES - PCM_HEADER_BYTES - MAX_RECOGNITION_BYTES) / 2),
);
const requestBudget = Object.freeze({
  max_input_bytes: MAX_INPUT_BYTES,
  max_evidence_bytes: 2000000,
  max_actions: 32,
  deadline_ms: 30000,
});
const utf8 = new TextEncoder();
const displayFormats = new Map([
  ["utf8-bytes-v2", { extension: "txt", mime: "text/plain;charset=utf-8", label: "回答をテキストで保存" }],
  ["png-image-display-v1", {
    extension: "png", mime: "image/png", label: "画像をPNGで保存",
    element: imagePreview, maxBytes: 6 * 1024 * 1024,
  }],
  ["wav-pcm16-mono-display-v1", {
    extension: "wav", mime: "audio/wav", label: "音声をWAVで保存",
    element: audioPreview, maxBytes: 48000 * 2 * 60 + 65536,
  }],
]);

let caseHistory = null;
let displayedRecord = null;
let verifiedPreview = null;
let forkSourceId = null;
let cancelAvailable = false;
let busy = false;
let downloading = false;
let scopeRevision = 0;
let webReadScope = null;
let webReadLoading = false;
let stagedAttachments = [];
let attachmentSelectionError = "";
let stagedSpeech = null;
let microphoneCapture = null;

function historyPager(id) {
  return {
    nav: document.getElementById(`${id}-pages`),
    previous: document.getElementById(`${id}-prev`),
    next: document.getElementById(`${id}-next`),
    label: document.getElementById(`${id}-page`),
  };
}

function setStatus(element, message, error = false) {
  element.textContent = message;
  element.hidden = !message;
  element.classList.toggle("is-error", error);
}

function currentScope() {
  const actorId = actorField.value.trim();
  const caseId = caseField.value.trim();
  const token = tokenField.value;
  if (!actorId || !caseId || !token) return null;
  return { actorId, caseId, token, revision: scopeRevision };
}

function isCurrent(scope) {
  const current = currentScope();
  return current !== null && current.revision === scope.revision
    && current.actorId === scope.actorId && current.caseId === scope.caseId
    && current.token === scope.token;
}

function updateControls() {
  const recording = microphoneCapture !== null;
  const microphoneDraft = stagedSpeech?.fromMicrophone === true;
  const conversationBusy = busy || recording;
  webReadButton.disabled = conversationBusy || webReadLoading;
  webReadHosts.disabled = conversationBusy;
  for (const field of [actorField, caseField, tokenField, speechClearButton]) {
    field.disabled = busy;
  }
  for (const field of [requestField, responseLocaleField, openCaseButton, attachmentField, attachmentClearButton,
    submitButton, forkCaseField, forkSubmitButton]) {
    field.disabled = conversationBusy;
  }
  speechFileField.disabled = conversationBusy || microphoneDraft;
  microphoneStart.disabled = busy || recording || stagedSpeech !== null;
  microphoneStop.disabled = !microphoneCapture?.node || microphoneCapture.stopping;
  microphoneCancel.disabled = busy || (!recording && !microphoneDraft);
  speechSendButton.disabled = busy || recording || stagedSpeech === null;
  speechSendButton.textContent = microphoneDraft ? "録音した発話を送信" : "WAVの発話を送信";
  speechClearButton.hidden = stagedSpeech === null || microphoneDraft;
  cancelCaseButton.disabled = busy || recording || !cancelAvailable;
  forkCurrentButton.disabled = conversationBusy;
  forkCurrentButton.hidden = displayedRecord?.lifecycle !== "COMMIT"
    || caseHistory?.can_fork === false;
  const output = downloadableRender(displayedRecord);
  downloadAnswerButton.hidden = output === null;
  downloadAnswerButton.disabled = busy || downloading
    || Boolean(output?.format.element && verifiedPreview?.record !== displayedRecord);
  if (output) downloadAnswerButton.textContent = output.format.label;
  for (const action of ownHistory.querySelectorAll("button")) action.disabled = conversationBusy;
  for (const removal of attachmentList.querySelectorAll("button")) removal.disabled = conversationBusy;

  const ownPage = caseHistory?.page || 1;
  const inheritedPage = caseHistory?.inherited_page || 1;
  ownPager.previous.disabled = conversationBusy || ownPage === 1;
  ownPager.next.disabled = conversationBusy || !caseHistory?.has_more;
  inheritedPager.previous.disabled = conversationBusy || inheritedPage === 1;
  inheritedPager.next.disabled = conversationBusy || !caseHistory?.inherited_has_more;
}

function setBusy(value) {
  busy = value;
  updateControls();
}

function clearDisplayedResult() {
  displayedRecord = null;
  clearMediaPreview();
  showRecognition(null);
  resultState.textContent = "待機中";
  delete resultState.dataset.state;
  resultMeta.replaceChildren();
  answerText.textContent = "";
  reasonText.textContent = "";
  reasonText.hidden = true;
  setStatus(attachmentGuidance, "");
  setStatus(downloadAnswerStatus, "");
  dependencyList.replaceChildren();
  recordJson.textContent = "";
  streamEvents.replaceChildren();
  streamEvents.hidden = true;
  loadingState.hidden = true;
  resultContent.hidden = true;
  emptyState.hidden = false;
}

function clearHistory(message) {
  caseHistory = null;
  ownHistory.replaceChildren();
  inheritedHistory.replaceChildren();
  ownPager.nav.hidden = true;
  inheritedPager.nav.hidden = true;
  setStatus(branchAnchor, "");
  setStatus(caseHistoryStatus, message);
  forkSourceId = null;
  forkSource.textContent = "";
  forkCaseField.value = "";
  forkForm.hidden = true;
  setStatus(forkStatus, "");
}

function resetCaseView() {
  scopeRevision += 1;
  webReadScope = null;
  webReadHosts.querySelectorAll("label").forEach((label) => label.remove());
  webReadHosts.hidden = true;
  setStatus(webReadStatus, "");
  cancelAvailable = false;
  requestField.value = "";
  clearAttachments();
  clearSpeechSelection();
  setStatus(formError, "");
  clearDisplayedResult();
  clearHistory("会話IDを指定して「会話を開く」を選択してください。");
  updateControls();
}

function attachmentKind(name) {
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || name.trim() !== name || utf8.encode(name).length > 1024
      || /[\p{Cc}/\\:<>"|?*\u202a-\u202e\u2066-\u2069]/u.test(name)) return null;
  const extension = name.slice(dot + 1).toLowerCase();
  if (["csv", "txt", "md"].includes(extension)) return "text";
  if (["pdf", "xlsx", "png", "jpg", "jpeg"].includes(extension)) return "binary";
  return null;
}

function attachmentBytes({ file, kind }) {
  const payload = kind === "binary" ? 4 * Math.ceil(file.size / 3) : file.size;
  return utf8.encode(file.name).length + payload;
}

function renderAttachments() {
  attachmentList.replaceChildren();
  stagedAttachments.forEach(({ file }, index) => {
    const item = document.createElement("li");
    item.dataset.fileName = file.name;
    const label = document.createElement("span");
    label.textContent = file.name;
    const remove = document.createElement("button");
    remove.className = "secondary-button";
    remove.type = "button";
    remove.dataset.action = "remove-attachment";
    remove.textContent = "外す";
    remove.setAttribute("aria-label", `${file.name} を添付から外す`);
    remove.addEventListener("click", () => {
      if (busy) return;
      stagedAttachments.splice(index, 1);
      renderAttachments();
      setStatus(formError, "");
      attachmentField.focus();
    });
    item.append(label, remove);
    attachmentList.append(item);
  });
  attachmentList.hidden = stagedAttachments.length === 0;
  attachmentClearButton.hidden = !attachmentSelectionError && stagedAttachments.length === 0;
  attachmentField.setAttribute("aria-invalid", String(Boolean(attachmentSelectionError)));
  setStatus(attachmentStatus, attachmentSelectionError
    || (stagedAttachments.length ? `次の依頼に${stagedAttachments.length}件を添付します。` : ""),
  Boolean(attachmentSelectionError));
  updateControls();
}

function clearAttachments() {
  attachmentField.value = "";
  stagedAttachments = [];
  attachmentSelectionError = "";
  renderAttachments();
}

function stageAttachments() {
  const selected = [...attachmentField.files];
  attachmentField.value = "";
  if (busy || selected.length === 0) return;
  const next = [
    ...stagedAttachments,
    ...selected.map((file) => ({ file, kind: attachmentKind(file.name) })),
  ];
  const names = new Set();
  let inputBytes = 0;
  let problem = next.length > MAX_ATTACHMENTS ? "添付できるのは8件までです。" : "";
  for (const entry of next) {
    if (problem) break;
    const { file, kind } = entry;
    const key = file.name.toLowerCase();
    if (!kind || file.size === 0) {
      problem = `「${file.name}」は空か、添付できるファイル形式・名前に該当しません。`;
    } else if (names.has(key)) {
      problem = `「${file.name}」と同じ名前のファイルが選択されています。`;
    } else if (kind === "binary" && file.size > MAX_BINARY_ATTACHMENT_BYTES) {
      problem = `「${file.name}」のバイナリデータが上限192,000バイトを超えています。`;
    } else {
      names.add(key);
      inputBytes += attachmentBytes(entry);
    }
  }
  if (!problem && inputBytes >= MAX_INPUT_BYTES) {
    problem = "添付の合計が、依頼と添付に使える256,000バイトの上限に達しています。";
  }
  attachmentSelectionError = problem
    ? `${problem} 選び直すか、添付の選択を解除してください。` : "";
  if (!problem) {
    stagedAttachments = next;
    setStatus(formError, "");
  }
  renderAttachments();
}

function encodeBinary(bytes) {
  let binary = "";
  for (let start = 0; start < bytes.length; start += 16384) {
    binary += String.fromCharCode(...bytes.subarray(start, start + 16384));
  }
  return btoa(binary);
}

async function snapshotAttachment({ file, kind }) {
  const bytes = new Uint8Array(await file.arrayBuffer());
  if (bytes.length !== file.size) throw new Error("選択後にサイズが変わりました。");
  if (kind === "binary") {
    return { name: file.name, text: "", binary_base64: encodeBinary(bytes) };
  }
  const text = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
  if (!text.trim() || text.includes("\0")) throw new Error("空または制御文字を含むテキストです。");
  return { name: file.name, text };
}

function mayCancelCase(text) {
  // This guard only keeps draft files out of the known cancellation commands.
  // The original text is still sent for Core to decide its meaning.
  const command = text.trim().replace(/[。.!！?？]+$/u, "").trim().toLowerCase();
  return ["cancel", "stop", "中止", "停止", "キャンセル", "やめて"].includes(command);
}

function releaseMicrophone(capture) {
  if (capture !== microphoneCapture) return;
  microphoneCapture = null;
  window.clearTimeout(capture.timer);
  capture.policyCheck?.abort();
  capture.stream?.getTracks().forEach((track) => track.stop());
  if (capture.node) {
    capture.node.port.close();
    capture.node.disconnect();
    capture.source.disconnect();
  }
  if (capture.context) void capture.context.close().catch(() => {});
}

function clearSpeechSelection() {
  if (microphoneCapture) releaseMicrophone(microphoneCapture);
  speechFileField.value = "";
  stagedSpeech = null;
  setStatus(speechFileStatus, "");
  setStatus(microphoneStatus, "");
  updateControls();
}

function hasExpectedBrowserResources() {
  const scripts = document.scripts;
  return scripts.length === 1 && scripts[0].src === `${location.origin}/app.js`
    && !performance.getEntriesByType("resource").some(({ name }) =>
      /^https?:/u.test(name) && new URL(name).origin !== location.origin);
}

function hasRestrictedPolicy(response) {
  const value = response.headers.get("content-security-policy") || "";
  const directives = value.split(";").map((directive) => directive.trim());
  return ["script-src", "connect-src"].every((name) => {
    const matches = directives.filter((directive) => directive.split(/\s/u, 1)[0] === name);
    return matches.length === 1 && matches[0] === `${name} 'self'`;
  });
}

function activeMicrophone(capture) {
  if (microphoneCapture !== capture) return false;
  if (!isCurrent(capture.scope)) {
    clearSpeechSelection();
    return false;
  }
  if (hasExpectedBrowserResources()) return true;
  microphoneError(capture, "ミニドラ以外のスクリプトが見つかったため、録音を中止しました。");
  return false;
}

function microphoneError(capture, message) {
  if (capture !== microphoneCapture) return;
  releaseMicrophone(capture);
  setStatus(microphoneStatus, message, true);
  updateControls();
}

function encodeMicrophoneWav(samples) {
  const wave = new Uint8Array(PCM_HEADER_BYTES + samples.length * 2);
  const header = new DataView(wave.buffer);
  wave.set(utf8.encode("RIFF"), 0);
  header.setUint32(4, wave.length - 8, true);
  wave.set(utf8.encode("WAVEfmt "), 8);
  header.setUint32(16, 16, true);
  header.setUint16(20, 1, true); // PCM
  header.setUint16(22, 1, true); // Mono
  header.setUint32(24, SPEECH_SAMPLE_RATE, true);
  header.setUint32(28, SPEECH_SAMPLE_RATE * 2, true);
  header.setUint16(32, 2, true);
  header.setUint16(34, 16, true);
  wave.set(utf8.encode("data"), 36);
  header.setUint32(40, samples.length * 2, true);
  samples.forEach((value, index) => {
    const sample = Math.max(-1, Math.min(1, value));
    header.setInt16(PCM_HEADER_BYTES + index * 2, Math.round(sample * (sample < 0 ? 32768 : 32767)), true);
  });
  return new Blob([wave], { type: "audio/wav" });
}

function stageMicrophone(capture, { samples, full }) {
  if (!activeMicrophone(capture)) return;
  if (!(samples instanceof Float32Array) || !samples.length || samples.length > MAX_MICROPHONE_FRAMES) {
    microphoneError(capture, "録音できた音声がありません。マイクを確認して録音し直してください。");
    return;
  }
  releaseMicrophone(capture);
  stagedSpeech = { file: encodeMicrophoneWav(samples), fromMicrophone: true, scope: capture.scope };
  setStatus(microphoneStatus, full
    ? "録音は上限で停止しました。送信すると、この会話の発話になります。"
    : "録音しました。送信すると、この会話の発話になります。");
  updateControls();
  speechSendButton.focus();
}

function stopMicrophone() {
  const capture = microphoneCapture;
  if (!capture?.node || capture.stopping || !activeMicrophone(capture)) return;
  capture.stopping = true;
  window.clearTimeout(capture.timer);
  capture.node.port.postMessage("stop");
  capture.stream.getTracks().forEach((track) => track.stop());
  capture.timer = window.setTimeout(() => {
    microphoneError(capture, "録音を確定できませんでした。録音し直してください。");
  }, 1000);
  updateControls();
}

async function startMicrophone() {
  if (busy || microphoneCapture || stagedSpeech) return;
  if (!hasExpectedBrowserResources()) {
    setStatus(microphoneStatus, "ミニドラのローカル画面を確認できないため、マイクを開けません。", true);
    return;
  }
  const scope = currentScope();
  if (!scope) {
    setStatus(microphoneStatus, "Actor ID、会話ID、ローカルAPI tokenを入力してください。", true);
    return;
  }
  if (requestField.value.trim() || stagedAttachments.length || attachmentSelectionError) {
    setStatus(microphoneStatus, "音声は単独で送信します。依頼欄を空にし、資料の添付があれば選択を解除してください。", true);
    return;
  }
  if (!navigator.mediaDevices?.getUserMedia || typeof AudioContext !== "function" || typeof AudioWorkletNode !== "function") {
    setStatus(microphoneStatus, "このブラウザーではマイク録音を使えません。WAVファイルを選んで送信できます。", true);
    return;
  }
  audioPreview.pause();
  const capture = { scope, policyCheck: new AbortController() };
  microphoneCapture = capture;
  const policyError = "ローカル画面の安全設定を確認できないため、マイクを開けません。";
  setStatus(microphoneStatus, "録音を始める前に、ローカル画面を確認しています。");
  updateControls();
  capture.timer = window.setTimeout(() => microphoneError(capture, policyError), 3000);
  try {
    const response = await fetch("/", {
      cache: "no-store",
      credentials: "omit",
      redirect: "error",
      referrerPolicy: "no-referrer",
      signal: capture.policyCheck.signal,
    });
    void response.body?.cancel().catch(() => {});
    if (!activeMicrophone(capture)) return;
    if (!response.ok || !hasRestrictedPolicy(response)) {
      microphoneError(capture, policyError);
      return;
    }
    window.clearTimeout(capture.timer);
    capture.policyCheck = null;
    setStatus(microphoneStatus, "ブラウザーのマイク許可を待っています。");
    const stream = await navigator.mediaDevices.getUserMedia({
      audio: { channelCount: 1, echoCancellation: false, noiseSuppression: false, autoGainControl: false },
      video: false,
    });
    if (!activeMicrophone(capture)) {
      stream.getTracks().forEach((track) => track.stop());
      return;
    }
    capture.stream = stream;
    const releaseDeadline = performance.now() + 9000;
    for (const track of stream.getAudioTracks()) {
      track.addEventListener("ended", () => {
        if (!capture.stopping) microphoneError(capture, "マイクとの接続が切れました。録音し直してください。");
      }, { once: true });
    }
    capture.timer = window.setTimeout(() => {
      microphoneError(capture, "録音を開始できませんでした。録音し直してください。");
    }, 4000);
    const context = new AudioContext({ sampleRate: SPEECH_SAMPLE_RATE });
    capture.context = context;
    if (context.sampleRate !== SPEECH_SAMPLE_RATE) throw new Error("Unsupported capture sample rate");
    await context.audioWorklet.addModule("/microphone-worklet.js");
    if (!activeMicrophone(capture)) return;
    const source = context.createMediaStreamSource(stream);
    const node = new AudioWorkletNode(context, "minidora-microphone", {
      numberOfInputs: 1,
      numberOfOutputs: 1,
      channelCount: 1,
      channelCountMode: "explicit",
      processorOptions: { maxFrames: MAX_MICROPHONE_FRAMES },
    });
    capture.source = source;
    capture.node = node;
    node.port.onmessage = ({ data }) => stageMicrophone(capture, data);
    node.addEventListener("processorerror", () => {
      microphoneError(capture, "録音処理が止まりました。録音し直してください。");
    }, { once: true });
    source.connect(node);
    node.connect(context.destination); // The worklet supplies silence to the output.
    await context.resume();
    if (!activeMicrophone(capture)) return;
    window.clearTimeout(capture.timer);
    const clipTimeout = MAX_MICROPHONE_FRAMES / SPEECH_SAMPLE_RATE * 1000 + 500;
    capture.timer = window.setTimeout(stopMicrophone, Math.min(clipTimeout, releaseDeadline - performance.now()));
    setStatus(microphoneStatus, "録音中です。話し終えたら録音を止めてください。");
    updateControls();
    microphoneStop.focus();
  } catch (error) {
    if (capture.policyCheck) {
      microphoneError(capture, policyError);
    } else {
      microphoneError(capture, error?.name === "NotAllowedError"
        ? "マイクへのアクセスが許可されませんでした。WAVファイルを選んで送信できます。"
        : "マイクを使って録音できませんでした。接続を確認するか、WAVファイルを選んでください。");
    }
  }
}

function stageSpeech() {
  const selected = [...speechFileField.files];
  speechFileField.value = "";
  if (busy || microphoneCapture || stagedSpeech?.fromMicrophone || selected.length === 0) return;
  clearSpeechSelection();
  const [file] = selected;
  if (selected.length !== 1 || !/\.wav$/iu.test(file.name) || file.size <= PCM_HEADER_BYTES) {
    setStatus(speechFileStatus, "16 kHz・16 bit・モノラルのPCM WAVを1件選んでください。", true);
    return;
  }
  if (file.size > MAX_INPUT_BYTES) {
    setStatus(speechFileStatus, "WAVが入力上限の256,000バイトを超えています。短い録音を選んでください。", true);
    return;
  }
  stagedSpeech = { file, fromMicrophone: false };
  setStatus(speechFileStatus, `選択した録音 ${file.name}`);
  updateControls();
}

class CaseRequestError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

async function requestLocalJson(path, scope, request = {}) {
  const response = await fetch(path, {
    method: "POST",
    cache: "no-store",
    credentials: "omit",
    referrerPolicy: "no-referrer",
    headers: {
      Authorization: `Bearer ${scope.token}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ actor_id: scope.actorId, ...request }),
  });
  const payload = await response.json();
  if (response.status !== 200) {
    const message = typeof payload?.error === "string" ? payload.error : `HTTP ${response.status}`;
    throw new CaseRequestError(response.status, message);
  }
  return payload;
}

async function requestCase(path, scope, request) {
  const payload = await requestLocalJson(path, scope, request);
  if (payload?.api_version !== "minidora-cases/v1") {
    throw new Error("会話APIの応答版を確認できません。");
  }
  return payload;
}

function clearWebReadSelection() {
  for (const checkbox of webReadHosts.querySelectorAll("input:checked")) checkbox.checked = false;
}

async function loadWebReadHosts() {
  const scope = currentScope();
  if (!scope) {
    setStatus(webReadStatus, "Actor ID、会話ID、ローカルAPI tokenを入力してください。", true);
    return;
  }
  webReadScope = null;
  webReadHosts.querySelectorAll("label").forEach((label) => label.remove());
  webReadHosts.hidden = true;
  webReadLoading = true;
  updateControls();
  setStatus(webReadStatus, "接続先の設定を確認しています。");
  try {
    const { read_hosts: hosts } = await requestLocalJson("/v1/read-hosts", scope);
    if (!isCurrent(scope)) return;
    if (!Array.isArray(hosts) || hosts.some((host) => typeof host !== "string" || !host.trim())) {
      throw new Error("接続先の設定を読み取れません。");
    }
    for (const host of hosts) {
      const label = document.createElement("label");
      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.value = host;
      label.append(checkbox, host);
      webReadHosts.append(label);
    }
    webReadScope = scope;
    webReadHosts.hidden = hosts.length === 0;
    setStatus(webReadStatus, hosts.length
      ? "" : "接続先が未設定です。サーバー起動時に --allow-read-host を指定してください。");
  } catch (error) {
    if (!isCurrent(scope)) return;
    const rejected = error instanceof CaseRequestError && [401, 403].includes(error.status);
    setStatus(webReadStatus, rejected
      ? "Actor ID とローカルAPI tokenを確認してください。"
      : "接続先を確認できません。ローカルサーバーの状態を確認してください。", true);
  } finally {
    webReadLoading = false;
    updateControls();
  }
}

function setConnection(connected, label) {
  connectionStatus.classList.toggle("is-connected", connected);
  connectionStatus.classList.toggle("is-offline", !connected);
  connectionStatus.lastElementChild.textContent = label;
}

async function checkConnection() {
  try {
    const response = await fetch("/health", {
      method: "GET",
      cache: "no-store",
      credentials: "omit",
      referrerPolicy: "no-referrer",
    });
    if (!response.ok) throw new Error("local Core unavailable");
    const health = await response.json();
    setConnection(true, `Core 接続中 · ${health.schema_version}`);
  } catch (_error) {
    setConnection(false, "Coreに接続できません");
  }
}

function makeInputId() {
  const suffix = typeof crypto.randomUUID === "function"
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  return `browser-${suffix}`;
}

function addMeta(label, value) {
  const chip = document.createElement("span");
  chip.className = "meta-chip";
  const strong = document.createElement("strong");
  strong.textContent = `${label} `;
  chip.append(strong, document.createTextNode(value));
  resultMeta.append(chip);
}

function showRecognition(recognition) {
  recognitionDetails.hidden = recognition === null;
  recognitionText.textContent = recognition?.text || "";
  recognitionRange.textContent = recognition
    ? `確認した原音 ${recognition.start_frame}〜${recognition.end_frame.toLocaleString("ja-JP")}フレーム（終点を含まない、${recognition.sample_rate.toLocaleString("ja-JP")} Hz）`
    : "";
}

function showRecord(record, recognition = null) {
  clearMediaPreview();
  displayedRecord = record;
  showRecognition(recognition);
  emptyState.hidden = true;
  loadingState.hidden = true;
  resultContent.hidden = false;
  resultState.textContent = record.lifecycle || "不明";
  resultState.dataset.state = record.lifecycle || "UNKNOWN";
  resultMeta.replaceChildren();
  addMeta("保証", record.guarantee || "UNKNOWN");
  addMeta("Input", record.input_id || "-");
  addMeta("Result", record.id || "-");
  const text = typeof record.text === "string" ? record.text : "";
  const reason = typeof record.reason === "string" ? record.reason : "";
  const permissionCode = "READ_PERMISSION_REQUIRED: ";
  const permissionStop = record.lifecycle === "SUSPEND" && reason.startsWith(permissionCode);
  answerText.textContent = permissionStop && (!text || text === reason.slice(permissionCode.length))
    ? "調査に使う接続先が許可されていません。" : text;
  reasonText.textContent = permissionStop
    ? "「オンライン調査」で接続先を選び、依頼を送り直してください。" : reason;
  reasonText.hidden = reason.length === 0;
  reasonText.classList.toggle("is-error", record.lifecycle === "REJECT" || record.lifecycle === "FAIL");
  setStatus(attachmentGuidance, /\bATTACHMENT_REQUIRED\b/u.test(reason)
    ? "「次の依頼に添付するファイル」から資料を選び、依頼を送ってください。" : "");
  setStatus(downloadAnswerStatus, "");
  dependencyList.replaceChildren();
  for (const dependency of Array.isArray(record.dependencies) ? record.dependencies : []) {
    if (typeof dependency !== "string") continue;
    const item = document.createElement("li");
    item.textContent = dependency;
    dependencyList.append(item);
  }
  const binaryDisplay = downloadableRender(record)?.format.element;
  recordJson.textContent = JSON.stringify(record, null, binaryDisplay ? 0 : 2);
  updateControls();
  showMediaPreview(record);
}

function downloadableRender(record) {
  const render = record?.render;
  const format = displayFormats.get(render?.renderer_version);
  const validId = (value) => typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
  if (record?.lifecycle !== "COMMIT" || !validId(record.id)
      || render?.status !== "RENDERED" || !format
      || render.target?.kind !== "DISPLAY" || !validId(render.plan_id)
      || render.plan_id !== record.content_plan?.id || !validId(render.actual_sha256)
      || !Array.isArray(render.bytes) || render.bytes.length === 0
      || render.bytes.length > (format.maxBytes ?? Infinity)) return null;
  return { render, format };
}

function validDisplayMedia(bytes, extension) {
  if (extension === "txt") return true;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (extension === "png") {
    if (bytes.length < 45 || view.getUint32(0) !== 0x89504e47 || view.getUint32(4) !== 0x0d0a1a0a
        || view.getUint32(8) !== 13 || view.getUint32(12) !== 0x49484452) return false;
    const width = view.getUint32(16);
    const height = view.getUint32(20);
    return width > 0 && width <= 2048 && height > 0 && height <= 2048;
  }
  if (bytes.length <= 44 || (bytes.length - 44) % 2 !== 0
      || view.getUint32(0) !== 0x52494646 || view.getUint32(4, true) !== bytes.length - 8
      || view.getUint32(8) !== 0x57415645 || view.getUint32(12) !== 0x666d7420
      || view.getUint32(16, true) !== 16 || view.getUint16(20, true) !== 1
      || view.getUint16(22, true) !== 1 || view.getUint16(32, true) !== 2
      || view.getUint16(34, true) !== 16 || view.getUint32(36) !== 0x64617461
      || view.getUint32(40, true) !== bytes.length - 44) return false;
  const sampleRate = view.getUint32(24, true);
  const sampleCount = (bytes.length - 44) / 2;
  return sampleRate >= 16000 && sampleRate <= 48000 && view.getUint32(28, true) === sampleRate * 2
    && sampleCount * 1000 >= sampleRate * 20 && sampleCount <= sampleRate * 60;
}

async function sha256Bytes(bytes) {
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

async function verifiedDisplayBlob({ render, format }) {
  for (const byte of render.bytes) {
    if (!Number.isInteger(byte) || byte < 0 || byte > 255) {
      throw new Error("回答データの形式が一致しません。");
    }
  }
  const bytes = Uint8Array.from(render.bytes);
  if (!validDisplayMedia(bytes, format.extension)) {
    throw new Error("画像または音声の形式が描画記録と一致しません。");
  }
  const sha256 = await sha256Bytes(bytes);
  if (sha256 !== render.actual_sha256) throw new Error("回答データのSHA-256が一致しません。");
  return new Blob([bytes], { type: format.mime });
}

function clearMediaPreview() {
  const old = verifiedPreview;
  verifiedPreview = null;
  if (audioPreview.hasAttribute("src")) {
    audioPreview.pause();
    audioPreview.removeAttribute("src");
    audioPreview.load();
  }
  imagePreview.removeAttribute("src");
  imagePreview.hidden = true;
  audioPreview.hidden = true;
  mediaPreview.hidden = true;
  if (old) URL.revokeObjectURL(old.url);
  setStatus(mediaPreviewStatus, "");
}

async function showMediaPreview(record) {
  const scope = currentScope();
  const output = downloadableRender(record);
  const element = output?.format.element;
  if (!scope || !element) return;
  setStatus(mediaPreviewStatus, "画像または音声の描画データを照合しています。");
  try {
    const blob = await verifiedDisplayBlob(output);
    if (!isCurrent(scope) || displayedRecord !== record) return;
    const url = URL.createObjectURL(blob);
    verifiedPreview = { record, scope, blob, url, element };
    element.src = url;
    element.hidden = false;
    mediaPreview.hidden = false;
    setStatus(mediaPreviewStatus, "");
  } catch (error) {
    if (!isCurrent(scope) || displayedRecord !== record) return;
    const reason = error instanceof Error ? error.message : "照合に失敗しました。";
    setStatus(mediaPreviewStatus, `画像または音声を表示できませんでした。 ${reason}`, true);
  } finally {
    updateControls();
  }
}

async function downloadAnswer() {
  if (busy || downloading) return;
  const scope = currentScope();
  const record = displayedRecord;
  const output = downloadableRender(record);
  if (!scope || !output) return;
  const preview = verifiedPreview;
  if (output.format.element && (preview?.record !== record || !isCurrent(preview.scope))) return;
  downloading = true;
  updateControls();
  setStatus(downloadAnswerStatus, "回答の保存データを照合しています。");
  try {
    const blob = output.format.element ? preview.blob : await verifiedDisplayBlob(output);
    if (!isCurrent(scope) || displayedRecord !== record) return;

    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = `minidora-${record.id.slice(0, 12)}.${output.format.extension}`;
    link.hidden = true;
    try {
      document.body.append(link);
      link.click();
      setStatus(downloadAnswerStatus, "ダウンロードをブラウザーに渡しました。");
    } finally {
      link.remove();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    }
  } catch (error) {
    if (!isCurrent(scope) || displayedRecord !== record) return;
    const reason = error instanceof Error ? error.message : "照合に失敗しました。";
    setStatus(downloadAnswerStatus, `回答を保存できませんでした。 ${reason}`, true);
  } finally {
    downloading = false;
    updateControls();
  }
}

function makeHistoryEntry(id, label, texts) {
  const item = document.createElement("li");
  item.dataset.resultId = id;
  const heading = document.createElement("div");
  heading.className = "history-entry-label";
  heading.textContent = label;
  item.append(heading);
  for (const text of texts) {
    if (typeof text !== "string" || !text) continue;
    const paragraph = document.createElement("p");
    paragraph.textContent = text;
    item.append(paragraph);
  }
  return item;
}

function historyAction(action, label, onClick) {
  const button = document.createElement("button");
  button.className = "secondary-button";
  button.dataset.action = action;
  button.type = "button";
  button.textContent = label;
  button.addEventListener("click", onClick);
  return button;
}

function updatePager(pager, page, hasMore) {
  pager.nav.hidden = page === 1 && !hasMore;
  pager.label.textContent = `${page}ページ`;
}

function chooseFork(resultId) {
  if (busy || caseHistory?.can_fork === false) return;
  if (forkSourceId !== resultId) {
    forkCaseField.value = "";
    setStatus(forkStatus, "");
  }
  forkSourceId = resultId;
  forkSource.textContent = resultId;
  forkForm.hidden = false;
  forkCaseField.focus();
}

function renderHistory(payload) {
  caseHistory = payload;
  const { entries, inherited, page, branch } = payload;
  if (page === 1) cancelAvailable = entries.length > 0 && entries[0].lifecycle !== "CANCEL";

  if (!payload.registered) {
    setStatus(caseHistoryStatus, "未登録の会話です。依頼を送るとこのIDで会話が始まります。");
  } else if (entries.length === 0 && page === 1) {
    setStatus(caseHistoryStatus, "この会話には保存された回答がありません。");
  } else {
    setStatus(caseHistoryStatus, `会話 ${payload.case_id} を開きました。`);
  }
  setStatus(branchAnchor, branch
    ? `分岐元 ${branch.source_case_id} · 結果ID ${branch.source_result_id} · 保存版 ${branch.source_revision}`
    : "");

  ownHistory.replaceChildren();
  for (const entry of entries) {
    const item = makeHistoryEntry(entry.id, `${entry.lifecycle} · ${entry.id}`, [entry.text, entry.reason]);
    const actions = document.createElement("div");
    actions.className = "history-entry-actions";
    actions.append(historyAction("open", "回答を開く", () => {
      loadHistory(page, payload.inherited_page, entry.id);
    }));
    if (payload.can_fork && entry.lifecycle === "COMMIT") {
      actions.append(historyAction("fork", "この回答から分岐", () => chooseFork(entry.id)));
    }
    item.append(actions);
    ownHistory.append(item);
  }
  inheritedHistory.replaceChildren();
  for (const entry of inherited) {
    inheritedHistory.append(makeHistoryEntry(
      entry.result_id,
      `${entry.source_case_id} · ${entry.result_id}`,
      [entry.input_text, entry.text],
    ));
  }

  updatePager(ownPager, page, payload.has_more);
  updatePager(inheritedPager, payload.inherited_page, payload.inherited_has_more);
  if (!payload.can_fork) {
    forkSourceId = null;
    forkSource.textContent = "";
    forkCaseField.value = "";
    forkForm.hidden = true;
  }
  updateControls();
}

async function loadHistory(page = 1, inheritedPage = 1, resultId = null) {
  if (busy) return;
  const scope = currentScope();
  if (!scope) {
    setStatus(caseHistoryStatus, "会話ID、Actor ID、ローカルAPI tokenを入力してください。", true);
    return;
  }
  setBusy(true);
  setStatus(caseHistoryStatus, "保存された履歴を確認しています。");
  try {
    const payload = await requestCase("/v1/cases/history", scope, {
      case_id: scope.caseId,
      page,
      inherited_page: inheritedPage,
      result_id: resultId,
    });
    if (!isCurrent(scope)) return;
    if (payload.case_id !== scope.caseId || payload.page !== page
        || payload.inherited_page !== inheritedPage
        || !Array.isArray(payload.entries) || !Array.isArray(payload.inherited)
        || (payload.branch && payload.branch.case_id !== scope.caseId)
        || (resultId !== null && payload.result?.id !== resultId)) {
      throw new Error("指定した会話・履歴と応答を照合できません。");
    }
    renderHistory(payload);
    if (payload.result) {
      streamEvents.replaceChildren();
      streamEvents.hidden = true;
      showRecord(payload.result);
      resultState.scrollIntoView({ block: "nearest" });
    }
  } catch (error) {
    if (!isCurrent(scope)) return;
    if (error instanceof CaseRequestError && [401, 403].includes(error.status)) resetCaseView();
    const reason = error instanceof Error ? error.message : "通信エラー";
    setStatus(caseHistoryStatus, `履歴を開けませんでした。 ${reason}`, true);
  } finally {
    setBusy(false);
  }
}

async function createFork(event) {
  event.preventDefault();
  if (busy || !forkSourceId || caseHistory?.can_fork === false) return;
  const scope = currentScope();
  const child = forkCaseField.value.trim();
  const resultId = forkSourceId;
  if (!scope || !child || child === scope.caseId) {
    setStatus(forkStatus, "元の会話とは異なる、新しい会話IDを入力してください。", true);
    return;
  }

  let opened = false;
  setBusy(true);
  setStatus(forkStatus, "分岐を作成しています。");
  try {
    const payload = await requestCase("/v1/cases/fork", scope, {
      source_case_id: scope.caseId,
      case_id: child,
      source_result_id: resultId,
    });
    if (!isCurrent(scope)) return;
    const branch = payload.branch;
    if (!branch || branch.case_id !== child || branch.source_case_id !== scope.caseId
        || branch.source_result_id !== resultId) {
      throw new Error("作成を依頼した分岐と応答を照合できません。");
    }
    caseField.value = child;
    resetCaseView();
    setStatus(forkStatus, `会話 ${child} に分岐しました。`);
    opened = true;
  } catch (error) {
    if (!isCurrent(scope)) return;
    if (error instanceof CaseRequestError && [401, 403].includes(error.status)) {
      resetCaseView();
      setStatus(caseHistoryStatus,
        `分岐先「${child}」を作成できませんでした。Actor ID、ローカルAPI token、会話の利用者を確認してください。 ${error.message}`,
        true);
    } else if (error instanceof CaseRequestError && error.status < 500) {
      setStatus(forkStatus, `分岐を作成できませんでした。 ${error.message}`, true);
    } else {
      setStatus(forkStatus, `分岐の成否を確認できません。会話ID「${child}」を入力欄に指定し、「会話を開く」で確認してください。`, true);
    }
  } finally {
    setBusy(false);
  }
  if (opened) await loadHistory();
}

function appendStreamEvent(label) {
  const item = document.createElement("li");
  item.textContent = label;
  streamEvents.append(item);
  streamEvents.hidden = false;
}

function validSpeechRecognition(recognition, expected) {
  return typeof recognition?.text === "string" && recognition.text.trim().length > 0
    && typeof recognition.recognizer === "string" && recognition.recognizer.trim().length > 0
    && recognition.wave_sha256 === expected.sha256
    && recognition.sample_rate === SPEECH_SAMPLE_RATE && recognition.start_frame === 0
    && Number.isSafeInteger(recognition.end_frame) && recognition.end_frame > 0
    && recognition.end_frame <= MAX_SPEECH_FRAMES
    && recognition.end_frame * 2 <= expected.byteLength - PCM_HEADER_BYTES;
}

async function consumeEventStream(response, expectedInputId, expectedSpeech = null) {
  if (!response.ok) {
    const payload = await response.json();
    throw new Error(typeof payload.error === "string" ? payload.error : `HTTP ${response.status}`);
  }
  if (!(response.headers.get("content-type") || "").startsWith("text/event-stream")) {
    throw new Error("unsupported response content type");
  }
  if (!response.body) throw new Error("stream body is unavailable");

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  let streamId = null;
  let provisionalSeen = false;
  let invalidatedSeen = false;
  let finalSeen = false;
  let finalRecord = null;
  let recognition = null;

  function receiveFrame(frame) {
    let eventName = "message";
    const data = [];
    for (const line of frame.split("\n")) {
      if (line.startsWith("event:")) eventName = line.slice(6).trim();
      if (line.startsWith("data:")) data.push(line.slice(5).trimStart());
    }
    if (data.length === 0) return;
    const payload = JSON.parse(data.join("\n"));
    if (payload.api_version !== "minidora-stream/v1" || typeof payload.stream_id !== "string") {
      throw new Error("unsupported stream event version");
    }
    if (payload.input_id !== expectedInputId && eventName === "provisional") {
      throw new Error("stream input identity differs from the submitted request");
    }
    if (streamId !== null && streamId !== payload.stream_id) {
      throw new Error("stream identity changed during delivery");
    }
    if (expectedSpeech && payload.stream_id !== expectedSpeech.streamId) {
      throw new Error("音声の入力IDと配信を照合できませんでした。");
    }
    streamId = payload.stream_id;

    if (eventName === "provisional") {
      if (provisionalSeen || payload.phase !== "PROVISIONAL" || payload.authority !== "NONE" || payload.content !== null) {
        throw new Error("invalid provisional stream event");
      }
      provisionalSeen = true;
      appendStreamEvent("暫定受付 · 回答内容はまだありません");
      return;
    }
    if (eventName === "invalidated") {
      if (!provisionalSeen || invalidatedSeen || finalSeen || payload.phase !== "INVALIDATED" || payload.authority !== "NONE") {
        throw new Error("invalid invalidation stream event");
      }
      invalidatedSeen = true;
      appendStreamEvent("暫定状態を無効化 · Coreの最終結果を待っています");
      return;
    }
    if (eventName === "final") {
      if (!provisionalSeen || finalSeen || !payload.result || payload.phase !== "FINAL") {
        throw new Error("invalid final stream event");
      }
      if (payload.result.input_id !== expectedInputId
          || (invalidatedSeen && payload.result.lifecycle === "COMMIT")
          || (!invalidatedSeen && payload.result.lifecycle !== "COMMIT")) {
        throw new Error("final result does not match its provisional stream state");
      }
      if (expectedSpeech) {
        if (!validSpeechRecognition(payload.recognition, expectedSpeech)) {
          throw new Error("認識した発話と送信した録音を照合できませんでした。");
        }
        recognition = payload.recognition;
      }
      finalSeen = true;
      finalRecord = payload.result;
      appendStreamEvent(`最終結果 · ${payload.result.lifecycle || "不明"}`);
      return;
    }
    if (eventName === "error") {
      throw new Error(typeof payload.error === "string" ? payload.error : "Core stream failed");
    }
    throw new Error(`unknown stream event: ${eventName}`);
  }

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    buffer = buffer.replace(/\r\n/g, "\n");
    let separator = buffer.indexOf("\n\n");
    while (separator >= 0) {
      receiveFrame(buffer.slice(0, separator));
      buffer = buffer.slice(separator + 2);
      separator = buffer.indexOf("\n\n");
    }
  }
  buffer += decoder.decode();
  if (buffer.trim()) receiveFrame(buffer);
  if (!finalSeen) throw new Error("stream ended before a final Core result");
  return { record: finalRecord, recognition };
}

function showTransportError(message, summary = "Coreの結果を受け取れませんでした。接続先とtokenを確認してください。") {
  clearMediaPreview();
  showRecognition(null);
  resultState.textContent = "HTTP ERROR";
  resultState.dataset.state = "FAIL";
  answerText.textContent = summary;
  reasonText.textContent = message;
  reasonText.hidden = false;
  reasonText.classList.add("is-error");
  setStatus(attachmentGuidance, "");
  setStatus(downloadAnswerStatus, "");
  resultMeta.replaceChildren();
  dependencyList.replaceChildren();
  displayedRecord = null;
  cancelAvailable = false;
  recordJson.textContent = "";
  emptyState.hidden = true;
  loadingState.hidden = true;
  resultContent.hidden = false;
}

function beginResultWait(message) {
  clearDisplayedResult();
  clearHistory("現在の保存済み履歴を読むには「会話を開く」を選択してください。");
  cancelAvailable = false;
  emptyState.hidden = true;
  loadingState.hidden = false;
  resultState.textContent = message;
}

async function sendSpeech() {
  if (busy || microphoneCapture || !stagedSpeech) return;
  const { file, fromMicrophone, scope: recordedScope } = stagedSpeech;
  if (fromMicrophone && !hasExpectedBrowserResources()) {
    clearSpeechSelection();
    setStatus(microphoneStatus, "ミニドラ以外のスクリプトが見つかったため、録音を破棄しました。", true);
    return;
  }
  if (recordedScope && !isCurrent(recordedScope)) {
    clearSpeechSelection();
    setStatus(microphoneStatus, "会話の変更により録音を破棄しました。", true);
    return;
  }
  const status = fromMicrophone ? microphoneStatus : speechFileStatus;
  const retry = fromMicrophone ? "録音し直してください。" : "WAVファイルを選び直してください。";
  const scope = currentScope();
  if (!scope) {
    setStatus(status, "Actor ID、会話ID、ローカルAPI tokenを入力してください。", true);
    return;
  }
  if (requestField.value.trim() || stagedAttachments.length || attachmentSelectionError) {
    setStatus(status, "音声は単独で送信します。依頼欄を空にし、資料の添付があれば選択を解除してください。", true);
    return;
  }
  const inputId = makeInputId();
  clearWebReadSelection();
  setBusy(true);
  speechSendButton.setAttribute("aria-busy", "true");
  clearSpeechSelection();
  setStatus(status, "録音を読み取っています。");
  let sending = false;
  try {
    const wave = new Uint8Array(await file.arrayBuffer());
    if (wave.length !== file.size) throw new Error("選択後に録音のサイズが変わりました。");
    const sha256 = await sha256Bytes(wave);
    if (!isCurrent(scope)) return;
    setStatus(status, "");
    beginResultWait("音声を認識中");
    sending = true;
    const response = await fetch("/v1/execute/wave/stream", {
      method: "POST",
      cache: "no-store",
      credentials: "omit",
      referrerPolicy: "no-referrer",
      headers: {
        Authorization: `Bearer ${scope.token}`,
        Accept: "text/event-stream",
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        actor_id: scope.actorId,
        case_id: scope.caseId,
        input_id: inputId,
        input_version: 1,
        wave: Array.from(wave),
        budget: requestBudget,
      }),
    });
    const { record, recognition } = await consumeEventStream(response, inputId, {
      sha256,
      byteLength: wave.length,
      streamId: `${scope.caseId}:${inputId}`,
    });
    if (!isCurrent(scope)) return;
    cancelAvailable = record.lifecycle !== "CANCEL";
    showRecord(record, recognition);
    setConnection(true, "Core 接続中");
  } catch (error) {
    if (!isCurrent(scope)) return;
    const reason = error instanceof Error ? error.message : "通信エラー";
    if (sending) {
      showTransportError(reason, "音声の依頼を完了できませんでした。");
      if (error instanceof TypeError) setConnection(false, "Coreに接続できません");
      setStatus(status, `もう一度音声を送るには、${retry}`);
    } else {
      setStatus(status, `録音は送信されていません。${retry} ${reason}`, true);
    }
  } finally {
    speechSendButton.removeAttribute("aria-busy");
    setBusy(false);
  }
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  if (busy || microphoneCapture) return;
  formError.hidden = true;
  formError.textContent = "";
  if (stagedSpeech?.fromMicrophone) {
    setStatus(formError, "未送信の録音があります。録音を送信するか破棄してから、本文を送信してください。", true);
    return;
  }

  const scope = currentScope();
  const text = requestField.value;
  if (!scope || !text.trim()) {
    formError.textContent = "Actor、Case、token、依頼を入力してください。";
    formError.hidden = false;
    return;
  }

  const cancellation = mayCancelCase(text);
  if (!cancellation && attachmentSelectionError) {
    setStatus(formError, "添付を選び直すか、添付の選択を解除してから送信してください。", true);
    return;
  }
  const files = cancellation ? [] : stagedAttachments;
  const inputBytes = files.reduce((total, entry) => total + attachmentBytes(entry), utf8.encode(text).length);
  if (inputBytes > MAX_INPUT_BYTES) {
    setStatus(formError, "依頼と添付の合計が256,000バイトを超えています。依頼を短くするか、一部の添付を外してください。", true);
    return;
  }

  const readHosts = !cancellation && webReadScope && isCurrent(webReadScope)
    ? Array.from(webReadHosts.querySelectorAll("input:checked"), (checkbox) => checkbox.value) : [];
  clearWebReadSelection();
  const envelope = {
    case_id: scope.caseId,
    input_id: makeInputId(),
    input_version: 1,
    actor_id: scope.actorId,
    text,
    locale: responseLocaleField.value,
    permissions: { read_hosts: readHosts, write_paths: [] },
    output_path: null,
    context_ref: null,
    budget: requestBudget,
  };

  setBusy(true);
  submitButton.setAttribute("aria-busy", "true");
  clearAttachments();
  clearSpeechSelection();
  let sending = false;
  try {
    if (files.length) {
      setStatus(attachmentStatus, "添付を読み取っています。");
      const attachments = [];
      for (const entry of files) {
        try {
          attachments.push(await snapshotAttachment(entry));
        } catch (error) {
          throw new Error(`「${entry.file.name}」を読み取れませんでした。`, { cause: error });
        }
        if (!isCurrent(scope)) return;
      }
      envelope.attachments = attachments;
      setStatus(attachmentStatus, "");
    }
    if (!isCurrent(scope)) return;
    beginResultWait("処理中");
    sending = true;
    const response = await fetch("/v1/execute/stream", {
      method: "POST",
      cache: "no-store",
      credentials: "omit",
      referrerPolicy: "no-referrer",
      headers: {
        Authorization: `Bearer ${scope.token}`,
        Accept: "text/event-stream",
        "Content-Type": "application/json",
      },
      body: JSON.stringify(envelope),
    });
    const { record } = await consumeEventStream(response, envelope.input_id);
    if (!isCurrent(scope)) return;
    cancelAvailable = record.lifecycle !== "CANCEL";
    showRecord(record);
    setConnection(true, "Core 接続中");
  } catch (error) {
    if (!isCurrent(scope)) return;
    if (!sending) {
      const reason = error instanceof Error ? error.message : "添付を読み取れませんでした。";
      attachmentSelectionError = `${reason} 依頼は送信されていません。形式や文字コードを確認して、添付を選び直してください。`;
      renderAttachments();
      return;
    }
    if (files.length) {
      setStatus(attachmentStatus, "添付の選択を解除しました。同じ依頼を送り直すときは、ファイルを選び直してください。");
    }
    showTransportError(error instanceof Error ? error.message : "通信エラー");
    if (error instanceof TypeError) setConnection(false, "Coreに接続できません");
  } finally {
    submitButton.removeAttribute("aria-busy");
    setBusy(false);
  }
});

cancelCaseButton.addEventListener("click", () => {
  if (cancelCaseButton.disabled) return;
  clearAttachments();
  clearSpeechSelection();
  requestField.value = "キャンセル";
  form.requestSubmit();
});

attachmentField.addEventListener("change", stageAttachments);
attachmentClearButton.addEventListener("click", () => {
  if (busy) return;
  clearAttachments();
  setStatus(formError, "");
  setStatus(attachmentStatus, "添付の選択を解除しました。");
  attachmentField.focus();
});
requestField.addEventListener("input", () => setStatus(formError, ""));
speechFileField.addEventListener("change", stageSpeech);
speechClearButton.addEventListener("click", () => {
  if (busy) return;
  clearSpeechSelection();
  speechFileField.focus();
});
speechSendButton.addEventListener("click", sendSpeech);
microphoneStart.addEventListener("click", startMicrophone);
microphoneStop.addEventListener("click", stopMicrophone);
microphoneCancel.addEventListener("click", () => {
  if (busy || (!microphoneCapture && !stagedSpeech?.fromMicrophone)) return;
  clearSpeechSelection();
  setStatus(microphoneStatus, "録音を破棄しました。");
  microphoneStart.focus();
});
window.addEventListener("pagehide", () => {
  clearSpeechSelection();
  clearWebReadSelection();
});

webReadButton.addEventListener("click", loadWebReadHosts);
for (const field of [actorField, caseField, tokenField]) {
  field.addEventListener("input", resetCaseView);
}
openCaseButton.addEventListener("click", () => loadHistory());
ownPager.previous.addEventListener("click", () => {
  if (caseHistory) loadHistory(caseHistory.page - 1, caseHistory.inherited_page);
});
ownPager.next.addEventListener("click", () => {
  if (caseHistory) loadHistory(caseHistory.page + 1, caseHistory.inherited_page);
});
inheritedPager.previous.addEventListener("click", () => {
  if (caseHistory) loadHistory(caseHistory.page, caseHistory.inherited_page - 1);
});
inheritedPager.next.addEventListener("click", () => {
  if (caseHistory) loadHistory(caseHistory.page, caseHistory.inherited_page + 1);
});
forkCurrentButton.addEventListener("click", () => {
  if (displayedRecord?.lifecycle === "COMMIT") chooseFork(displayedRecord.id);
});
downloadAnswerButton.addEventListener("click", downloadAnswer);
for (const element of [imagePreview, audioPreview]) {
  element.addEventListener("error", () => {
    if (verifiedPreview?.element === element && element.getAttribute("src") === verifiedPreview.url) {
      setStatus(mediaPreviewStatus, "このブラウザーではプレビューを開けませんでした。保存したファイルを対応するアプリで確認してください。", true);
    }
  });
}
forkForm.addEventListener("submit", createFork);

checkConnection();
