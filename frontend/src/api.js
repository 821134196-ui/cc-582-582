const USER_KEY = 'sls_user_id';

export function getUserId() {
  const v = localStorage.getItem(USER_KEY);
  return v ? Number(v) : null;
}
export function setUserId(id) {
  localStorage.setItem(USER_KEY, String(id));
}

export async function api(method, path, body) {
  const headers = { 'Content-Type': 'application/json' };
  const uid = getUserId();
  if (uid) headers['X-User-Id'] = uid;
  const res = await fetch('/api' + path, {
    method,
    headers,
    body: body ? JSON.stringify(body) : undefined,
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(data.error || `请求失败 (${res.status})`);
  return data;
}

export const STATUS_LABEL = {
  draft: '草稿',
  submitted: '待确认',
  confirmed: '已确认',
  returning: '归还中',
  completed: '已完成',
  cancelled: '已取消',
};
export const RESULT_LABEL = {
  confirmed: '已确认',
  rejected: '驳回',
  revoked: '已撤销',
};

export function fmtTime(s) {
  if (!s) return '—';
  const d = new Date(s.endsWith('Z') || s.includes('+') ? s : s.replace(' ', 'T') + 'Z');
  if (isNaN(d)) return s;
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getMonth() + 1}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

// <input type="datetime-local"> 需要的本地时间字符串
export function toLocalInput(s) {
  if (!s) return '';
  const d = new Date(s.endsWith('Z') || s.includes('+') ? s : s.replace(' ', 'T') + 'Z');
  if (isNaN(d)) return '';
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
}
export function fromLocalInput(s) {
  if (!s) return null;
  return new Date(s).toISOString();
}
