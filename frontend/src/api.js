// 极简 API 封装：当前用户通过 X-User-Id 头传递（本地模拟登录）
const listeners = [];

function getUser() {
  const raw = localStorage.getItem('uid');
  return raw ? Number(raw) : null;
}

export function setUser(id) {
  localStorage.setItem('uid', String(id));
  listeners.forEach((fn) => fn());
}

export function onChange(fn) {
  listeners.push(fn);
}

export async function api(path, options = {}) {
  const uid = getUser();
  const headers = { 'Content-Type': 'application/json', ...(options.headers || {}) };
  if (uid) headers['X-User-Id'] = String(uid);
  const res = await fetch('/api' + path, { ...options, headers });
  const text = await res.text();
  const data = text ? JSON.parse(text) : null;
  if (!res.ok) {
    let message = data?.error || `HTTP ${res.status}`;
    // 确认/提交接口把详细校验报告编码在 error JSON 字符串里
    let report = null;
    if (typeof message === 'string' && message.trimStart().startsWith('{')) {
      try {
        const parsed = JSON.parse(message);
        if (parsed && typeof parsed === 'object') {
          report = parsed.report || null;
          message = parsed.message || message;
        }
      } catch (_) {}
    }
    throw new ApiError(message, res.status, data, report);
  }
  return data;
}

export class ApiError extends Error {
  constructor(message, status, body, report = null) {
    super(message);
    this.status = status;
    this.body = body;
    this.report = report;
  }
}

export function fmtTime(iso) {
  if (!iso) return '—';
  const d = new Date(iso);
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getMonth() + 1}/${d.getDate()} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

export function toLocalInput(iso) {
  if (!iso) return '';
  const d = new Date(iso);
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
}

export function fromLocalInput(v) {
  if (!v) return null;
  return new Date(v).toISOString();
}
