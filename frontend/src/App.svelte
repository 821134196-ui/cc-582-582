<script>
  import { onMount } from 'svelte';
  import { api, getUserId, setUserId } from './api.js';
  import ProjectList from './ProjectList.svelte';
  import ProjectDetail from './ProjectDetail.svelte';
  import Timeline from './Timeline.svelte';
  import NotificationsPanel from './NotificationsPanel.svelte';

  let meta = null;
  let user = null;
  let tab = 'projects'; // projects | timeline | notifications
  let selectedProject = null;
  let notifications = [];
  let unread = 0;

  onMount(loadMeta);

  async function loadMeta() {
    meta = await api('GET', '/meta');
    const uid = getUserId();
    user = meta.users.find((u) => u.id === uid) || null;
    refreshNotifications();
  }

  function switchUser(id) {
    setUserId(id);
    user = meta.users.find((u) => u.id === Number(id)) || null;
    selectedProject = null;
    refreshNotifications();
  }

  async function refreshNotifications() {
    if (!user) { notifications = []; unread = 0; return; }
    try {
      notifications = await api('GET', '/notifications');
      unread = notifications.filter((n) => !n.read).length;
    } catch (e) {}
  }

  function openProject(p) {
    selectedProject = p.id;
    tab = 'projects';
  }
</script>

<header style="background:var(--panel);border-bottom:1px solid var(--line);padding:10px 20px;display:flex;align-items:center;gap:18px;">
  <strong style="font-size:16px;">🎛 摄影棚灯光排期</strong>
  <nav style="display:flex;gap:6px;">
    <button class:primary={tab==='projects' && !selectedProject}
            on:click={() => { tab='projects'; selectedProject=null; }}>项目方案</button>
    <button class:primary={tab==='timeline'} on:click={() => tab='timeline'}>占用时段</button>
    <button class:primary={tab==='notifications'} on:click={() => { tab='notifications'; refreshNotifications(); }}>
      通知{#if unread} ({unread}){/if}
    </button>
  </nav>
  <div style="margin-left:auto;display:flex;align-items:center;gap:10px;">
    {#if meta}
      <span class="muted small">当前身份</span>
      <select style="width:auto;" value={user?.id ?? ''} on:change={(e) => switchUser(e.target.value)}>
        <option value="" disabled>请选择…</option>
        {#each meta.users as u}
          <option value={u.id}>{u.name}（{u.role === 'crew' ? '场务' : '摄影师'}）</option>
        {/each}
      </select>
    {/if}
  </div>
</header>

<main style="padding:20px;max-width:1280px;margin:0 auto;">
  {#if !user}
    <div class="panel">
      <h2>欢迎使用摄影棚灯光方案与设备占用排期系统</h2>
      <p class="muted">请在右上角选择登录身份：摄影师提交拍摄方案、安排灯位；场务核对实物、回路负载并确认时段。</p>
      {#if meta}
        <div class="row">
          {#each meta.users as u}
            <button class="primary" on:click={() => switchUser(u.id)}>
              以 {u.name}（{u.role === 'crew' ? '场务' : '摄影师'}）进入
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {:else if tab === 'timeline'}
    <Timeline {meta} {openProject} />
  {:else if tab === 'notifications'}
    <NotificationsPanel {notifications} on:read={refreshNotifications} on:openProject={openProject} />
  {:else if selectedProject}
    <ProjectDetail
      projectId={selectedProject}
      {meta}
      {user}
      on_back={() => (selectedProject = null)}
    />
  {:else}
    <ProjectList {meta} {user} on:open={openProject} />
  {/if}
</main>
