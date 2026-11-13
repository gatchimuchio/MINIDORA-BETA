"use strict";

const form = document.querySelector("#request-form");
const actorField = document.querySelector("#actor-id");
const caseField = document.querySelector("#case-id");
const tokenField = document.querySelector("#api-token");
const requestField = document.querySelector("#request-text");
const submitButton = document.querySelector("#submit-button");
const connectionStatus = document.querySelector("#connection-status");
const formError = document.querySelector("#form-error");
const resultState = document.querySelector("#result-state");
const emptyState = document.querySelector("#empty-state");
const loadingState = document.querySelector("#loading-state");
const resultContent = document.querySelector("#result-content");
const resultMeta = document.querySelector("#result-meta");
const answerText = document.querySelector("#answer-text");
const reasonText = document.querySelector("#reason-text");
const dependencyList = document.querySelector("#dependency-list");
const recordJson = document.querySelector("#record-json");

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

function showRecord(record) {
  emptyState.hidden = true;
  loadingState.hidden = true;
  resultContent.hidden = false;
  resultState.textContent = record.lifecycle || "不明";
  resultState.dataset.state = record.lifecycle || "UNKNOWN";
  resultMeta.replaceChildren();
  addMeta("保証", record.guarantee || "UNKNOWN");
  addMeta("Input", record.input_id || "-");
  addMeta("Result", record.id || "-");
  answerText.textContent = typeof record.text === "string" ? record.text : "";
  const reason = typeof record.reason === "string" ? record.reason : "";
  reasonText.textContent = reason;
  reasonText.hidden = reason.length === 0;
  reasonText.classList.toggle("is-error", record.lifecycle === "REJECT" || record.lifecycle === "FAIL");
  dependencyList.replaceChildren();
  for (const dependency of Array.isArray(record.dependencies) ? record.dependencies : []) {
    if (typeof dependency !== "string") continue;
    const item = document.createElement("li");
    item.textContent = dependency;
    dependencyList.append(item);
  }
  recordJson.textContent = JSON.stringify(record, null, 2);
}

function showTransportError(message) {
  resultState.textContent = "HTTP ERROR";
  resultState.dataset.state = "FAIL";
  answerText.textContent = "Coreの結果を受け取れませんでした。接続先とtokenを確認してください。";
  reasonText.textContent = message;
  reasonText.hidden = false;
  reasonText.classList.add("is-error");
  resultMeta.replaceChildren();
  dependencyList.replaceChildren();
  recordJson.textContent = "";
  emptyState.hidden = true;
  loadingState.hidden = true;
  resultContent.hidden = false;
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  formError.hidden = true;
  formError.textContent = "";

  const actor = actorField.value.trim();
  const caseId = caseField.value.trim();
  const token = tokenField.value;
  const text = requestField.value.trim();
  if (!actor || !caseId || !token || !text) {
    formError.textContent = "Actor、Case、token、依頼を入力してください。";
    formError.hidden = false;
    return;
  }

  const envelope = {
    case_id: caseId,
    input_id: makeInputId(),
    input_version: 1,
    actor_id: actor,
    text,
    locale: "ja",
    permissions: { read_hosts: [], write_paths: [] },
    output_path: null,
    context_ref: null,
    budget: {
      max_input_bytes: 256000,
      max_evidence_bytes: 2000000,
      max_actions: 32,
      deadline_ms: 30000,
    },
  };

  submitButton.disabled = true;
  submitButton.setAttribute("aria-busy", "true");
  emptyState.hidden = true;
  resultContent.hidden = true;
  loadingState.hidden = false;
  resultState.textContent = "処理中";
  delete resultState.dataset.state;
  try {
    const response = await fetch("/v1/execute", {
      method: "POST",
      cache: "no-store",
      credentials: "omit",
      referrerPolicy: "no-referrer",
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(envelope),
    });
    const payload = await response.json();
    if (!response.ok) {
      throw new Error(typeof payload.error === "string" ? payload.error : `HTTP ${response.status}`);
    }
    showRecord(payload);
    setConnection(true, "Core 接続中");
  } catch (error) {
    showTransportError(error instanceof Error ? error.message : "通信エラー");
    if (error instanceof TypeError) setConnection(false, "Coreに接続できません");
  } finally {
    submitButton.disabled = false;
    submitButton.removeAttribute("aria-busy");
  }
});

checkConnection();
