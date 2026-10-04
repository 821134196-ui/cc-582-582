<script>
  import { createEventDispatcher } from 'svelte';
  import { api, fmtTime } from './api.js';

  export let notifications;
  const dispatch = createEventDispatcher();

  const KIND = {
    project: '项目', submit: '提交', reject: '驳回',
    replan: '变更', cancel: '取消', damage: '损坏',
    waiver: '豁免', power: '功率',
  };

  async function mark(n) {
    if (n.read) return;
    await api('POST', `/notifications/${n.id}/read`);
    dispatch('read');
  }
</script>

<div class="panel">
  <h2>通知</h2>
  {#each notifications as n}
    <div class="n-item" class:unread={!n.read} on:click={() => mark(n)}>
      <span class="tag">{KIND[n.kind] ?? n.kind}</span>
      <span style="flex:1;">{n.message}</span>
      <span class="muted small">{fmtTime(n.created_at)}</span>
    </div>
  {/each}
  {#if notifications.length === 0}<p class="muted">暂无通知</p>{/if}
</div>

<style>
  .n-item { display: flex; align-items: center; gap: 12px; padding: 10px 6px; border-bottom: 1px solid var(--line); cursor: pointer; }
  .n-item.unread { background: rgba(79,140,255,.07); border-radius: 6px; }
</style>
