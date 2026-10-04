import { writable, derived, get } from 'svelte/store';
import { api, setUser as persistUser, onChange } from './api.js';

export const users = writable([]);
export const currentUserId = writable(localStorage.getItem('uid') ? Number(localStorage.getItem('uid')) : null);
export const studios = writable([]);

export const currentUser = derived([users, currentUserId], ([$users, $id]) =>
  $users.find((u) => u.id === $id) || null
);

onChange(() => currentUserId.set(localStorage.getItem('uid') ? Number(localStorage.getItem('uid')) : null));

export function switchUser(id) {
  persistUser(id);
}

export async function loadUsers() {
  users.set(await api('/users'));
}
export async function loadStudios() {
  studios.set(await api('/studios'));
}

// ---------------- Toast ----------------
export const toasts = writable([]);
let toastSeq = 1;
export function toast(message, kind = 'info', timeout = 4200) {
  const id = toastSeq++;
  toasts.update((t) => [...t, { id, message, kind }]);
  if (timeout > 0) setTimeout(() => toasts.update((t) => t.filter((x) => x.id !== id)), timeout);
}

export const STATUS_LABELS = {
  draft: '草稿',
  pending: '待场务确认',
  confirmed: '已确认（资源已锁定）',
  active: '拍摄中',
  changes_requested: '需修改',
  rejected: '已驳回',
  completed: '已完成',
  cancelled: '已取消',
};

export function studioName(id) {
  return get(studios).find((s) => s.id === id)?.name || `场地#${id}`;
}
export function userName(id) {
  return get(users).find((u) => u.id === id)?.name || `用户#${id}`;
}
