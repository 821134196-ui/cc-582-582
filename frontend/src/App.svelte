<script>
  import { onMount, onDestroy } from 'svelte';
  import { users, currentUser, currentUserId, switchUser, loadUsers, loadStudios, toasts, toast } from './stores.js';
  import { api } from './api.js';
  import ProjectsView from './views/ProjectsView.svelte';
  import TimelineView from './views/TimelineView.svelte';
  import PowerView from './views/PowerView.svelte';
  import ProjectDetail from './views/ProjectDetail.svelte';
  import Toasts from './components/Toasts.svelte';

  let tab = 'projects';
  let detailId = null;
  let notifications = [];
  let showNotifs = false;
  let unread = 0;
  let pollTimer = null;

  onMount(async () => {
    await loadUsers();
    await loadStudios();
    refreshNotifs();
    pollTimer = setInterval(refreshNotifs, 8000);
    window.addEventListener('open-project', onOpenProjectEvent);
  });
  onDestroy(() => {
    clearInterval(pollTimer);
    window.removeEventListener('open-project', onOpenProjectEvent);
  });

  function onOpenProjectEvent(e) {
    tab = 'projects';
    openProject(e.detail);
  }

  $: if (!$currentUserId && $users.length) switchUser($users[0].id);

  async function refreshNotifs() {
    if (!localStorage.getItem('uid')) return;
    try {
      notifications = await api('/notifications');
      unread = notifications.filter((n) => !n.is_read).length;
    } catch (_) {}
  }

  async function markAllRead() {
    await api('/notifications', { method: 'POST' });
    refreshNotifs();
  }

  function openProject(id) {
    detailId = id;
  }
  function closeDetail() {
    detailId = null;
  }
</script>

<Toasts />

<header class="topbar">
  <h1>🎬 摄影棚灯光排期</h1>
  <div class="tabs">
    <button class:active={tab === 'projects' && !detailId} on:click={() => { tab = 'projects'; detailId = null; }}>方案与项目</button>
    <button class:active={tab === 'timeline'} on:click={() => { tab = 'timeline'; detailId = null; }}>排期时间线</button>
    <button class:active={tab === 'power'} on:click={() => { tab = 'power'; detailId = null; }}>回路功率（模拟电表）</button>
  </div>
  <div class="spacer"></div>

  <div class="rel">
    <button class="btn ghost sm" on:click={() => (showNotifs = !showNotifs)}>
      🔔 通知{#if unread > 0}<span class="dot">{unread}</span>{/if}
    </button>
    {#if showNotifs}
      <div class="notif-pop">
        <div class="row" style="padding:8px 12px;justify-content:space-between">
          <strong>通知（本地模拟推送）</strong>
          <a on:click={markAllRead}>全部已读</a>
        </div>
        {#each notifications as n (n.id)}
          <a
            class="notif-item {n.is_read ? '' : 'unread'}"
            on:click={() => {
              api(`/notifications/${n.id}/read`, { method: 'POST' }).then(refreshNotifs);
              if (n.project_id) openProject(n.project_id);
              showNotifs = false;
            }}
          >
            <div class="t">{n.title}</div>
            <div class="b">{n.body}</div>
          </a>
        {:else}
          <div class="muted small" style="padding:14px">暂无通知</div>
        {/each}
      </div>
    {/if}
  </div>

  <select
    value={$currentUserId ?? ''}
    on:change={(e) => switchUser(Number(e.target.value))}
    title="切换当前登录身份（本地模拟）"
  >
    {#each $users as u}
      <option value={u.id}>{u.name}（{u.role === 'photographer' ? '摄影师' : u.role === 'crew' ? '场务' : '管理员'}）</option>
    {/each}
  </select>
</header>

<main>
  {#if detailId}
    <ProjectDetail id={detailId} on:close={closeDetail} on:changed={() => { refreshNotifs(); }} />
  {:else if tab === 'projects'}
    <ProjectsView on:open={(e) => openProject(e.detail)} />
  {:else if tab === 'timeline'}
    <TimelineView />
  {:else if tab === 'power'}
    <PowerView />
  {/if}
</main>
