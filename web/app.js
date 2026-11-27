'use strict';

(() => {
  const $ = (sel) => document.querySelector(sel);
  const $$ = (sel) => Array.from(document.querySelectorAll(sel));

  const state = {
    role: null,
    credential: null,
    ws: null,
    leaving: false,
    reconnectTimer: null,
    me: { id: null, name: '', role: '' },
    participants: new Map(),
    presenter: null,
    policyEveryone: false,
    canEdit: false,
    iceServers: [],
    pcs: new Map(),
    remoteStreams: new Map(),
    streamKinds: new Map(),
    local: { screen: null, cam: null, mic: null },
    strokes: [],
    annotating: false,
    liveStroke: null,
    chat: [],
    inviteUrl: '',
    clipboardText: '',
    lastPointer: 0,
  };

  const els = {};
  const audioEls = new Map();
  const cursors = new Map();

  /* ------------------------------------------------------------------ utils */

  function toast(message, opts = {}) {
    const wrap = document.createElement('div');
    wrap.className = 'toast';
    const text = document.createElement('span');
    text.textContent = message;
    wrap.appendChild(text);
    const actions = opts.actions || [];
    for (const action of actions) {
      const button = document.createElement('button');
      button.className = 'btn tiny';
      button.textContent = action.label;
      button.addEventListener('click', () => {
        try { action.onClick(); } finally { wrap.remove(); }
      });
      wrap.appendChild(button);
    }
    els.toasts.appendChild(wrap);
    const timeout = opts.timeout ?? (actions.length ? 15000 : 5200);
    if (timeout > 0) {
      setTimeout(() => {
        wrap.classList.add('leaving');
        setTimeout(() => wrap.remove(), 220);
      }, timeout);
    }
    return wrap;
  }

  function send(obj) {
    if (state.ws && state.ws.readyState === WebSocket.OPEN) {
      state.ws.send(JSON.stringify(obj));
      return true;
    }
    return false;
  }

  function initials(name) {
    return (name || '?').trim().slice(0, 2).toUpperCase();
  }

  function isSecure() {
    return window.isSecureContext || location.hostname === 'localhost' || location.hostname === '127.0.0.1';
  }

  function mediaError(error, what) {
    console.warn(what, error);
    let hint = error && error.name ? error.name : String(error);
    if (!isSecure()) {
      hint = 'This page is not a secure context. Open the HTTPS invite link (or localhost) to capture media.';
    } else if (hint === 'NotAllowedError') {
      hint = 'Permission was denied. Allow access in the browser prompt and try again.';
    } else if (hint === 'NotFoundError' || hint === 'DevicesNotFoundError') {
      hint = 'No matching camera or microphone was found.';
    } else if (hint === 'NotReadableError') {
      hint = 'The device is already in use by another application.';
    }
    toast(`${what} failed: ${hint}`, { timeout: 8000 });
  }

  /* ------------------------------------------------------------------- boot */

  async function init() {
    els.joinScreen = $('#joinScreen');
    els.joinForm = $('#joinForm');
    els.joinName = $('#joinName');
    els.joinTitle = $('#joinTitle');
    els.joinBlurb = $('#joinBlurb');
    els.joinError = $('#joinError');
    els.app = $('#app');
    els.connStatus = $('#connStatus');
    els.peopleStrip = $('#peopleStrip');
    els.inviteBtn = $('#inviteBtn');
    els.leaveBtn = $('#leaveBtn');
    els.stage = $('#stage');
    els.stageEmpty = $('#stageEmpty');
    els.stageEmptyTitle = $('#stageEmptyTitle');
    els.stageEmptyHint = $('#stageEmptyHint');
    els.screenVideo = $('#screenVideo');
    els.canvas = $('#annotationCanvas');
    els.reactionLayer = $('#reactionLayer');
    els.cursorLayer = $('#cursorLayer');
    els.stageBadges = $('#stageBadges');
    els.selfWrap = $('#selfWrap');
    els.selfVideo = $('#selfVideo');
    els.selfLabel = $('#selfLabel');
    els.controlBanner = $('#controlBanner');
    els.controlBannerText = $('#controlBannerText');
    els.controlRequestBtn = $('#controlRequestBtn');
    els.toasts = $('#toasts');
    els.clipboardToast = $('#clipboardToast');
    els.clipboardFrom = $('#clipboardFrom');
    els.clipboardPreview = $('#clipboardPreview');
    els.clipboardCopy = $('#clipboardCopy');
    els.clipboardClose = $('#clipboardClose');
    els.filmstrip = $('#filmstrip');
    els.codeArea = $('#codeArea');
    els.codeStatus = $('#codeStatus');
    els.codeOpen = $('#codeOpen');
    els.codeCopy = $('#codeCopy');
    els.codeDownload = $('#codeDownload');
    els.codeFile = $('#codeFile');
    els.chatLog = $('#chatLog');
    els.chatForm = $('#chatForm');
    els.chatInput = $('#chatInput');
    els.peopleList = $('#peopleList');
    els.hostControls = $('#hostControls');
    els.policyEveryone = $('#policyEveryone');
    els.shareBtn = $('#shareBtn');
    els.camBtn = $('#camBtn');
    els.micBtn = $('#micBtn');
    els.annotateBtn = $('#annotateBtn');
    els.penColor = $('#penColor');
    els.clearAnno = $('#clearAnno');
    els.clipSend = $('#clipSend');

    const path = location.pathname;
    if (path.startsWith('/h/')) {
      state.role = 'host';
      state.credential = decodeURIComponent(path.slice(3));
    } else if (path.startsWith('/j/')) {
      state.role = 'guest';
      state.credential = decodeURIComponent(path.slice(3));
    } else {
      location.replace('/');
      return;
    }

    try {
      const response = await fetch('/api/config', { cache: 'no-store' });
      if (response.ok) {
        const config = await response.json();
        state.iceServers = Array.isArray(config.iceServers) ? config.iceServers : [];
      }
    } catch (error) {
      console.warn('config', error);
    }

    els.joinTitle.textContent = state.role === 'host' ? 'Start your pairing session' : 'Join the pairing session';
    els.joinBlurb.textContent = state.role === 'host'
      ? 'Share your screen, edit code together, and hand out a single unguessable link.'
      : 'You were invited to pair. Pick a name and join — no account needed.';
    const saved = sessionStorage.getItem('triple.name');
    els.joinName.value = saved || (state.role === 'host' ? 'Host' : '');
    els.joinName.focus();
    els.joinName.select();

    els.joinForm.addEventListener('submit', (event) => {
      event.preventDefault();
      const name = els.joinName.value.trim() || (state.role === 'host' ? 'Host' : 'Guest');
      sessionStorage.setItem('triple.name', name);
      state.me.name = name;
      els.joinScreen.hidden = true;
      els.app.hidden = false;
      resizeCanvas();
      connect(name);
    });

    wireUi();
    window.addEventListener('resize', resizeCanvas);
  }

  /* -------------------------------------------------------------------- ui */

  function wireUi() {
    $$('.tab').forEach((tab) => {
      tab.addEventListener('click', () => {
        $$('.tab').forEach((t) => t.classList.toggle('active', t === tab));
        $$('.tabpane').forEach((pane) => pane.classList.toggle('active', pane.id === `tab-${tab.dataset.tab}`));
      });
    });

    els.leaveBtn.addEventListener('click', () => {
      if (!confirm('Leave this session?')) return;
      state.leaving = true;
      stopAllMedia();
      closeAllPeers();
      if (state.ws) state.ws.close();
      location.href = '/';
    });

    els.inviteBtn.addEventListener('click', async () => {
      if (!state.inviteUrl) return;
      try {
        await navigator.clipboard.writeText(state.inviteUrl);
        toast('Invite link copied');
      } catch (error) {
        window.prompt('Copy this invite link', state.inviteUrl);
      }
    });

    els.shareBtn.addEventListener('click', toggleScreenShare);
    els.camBtn.addEventListener('click', toggleCamera);
    els.micBtn.addEventListener('click', toggleMic);

    els.annotateBtn.addEventListener('click', () => {
      state.annotating = !state.annotating;
      els.annotateBtn.classList.toggle('active', state.annotating);
      els.canvas.classList.toggle('active', state.annotating);
      els.annotateBtn.textContent = state.annotating ? 'Stop drawing' : 'Annotate';
    });

    els.clearAnno.addEventListener('click', () => send({ type: 'annotation_clear' }));

    els.canvas.addEventListener('pointerdown', onPointerDown);
    els.canvas.addEventListener('pointermove', onPointerMove);
    els.canvas.addEventListener('pointerup', onPointerUp);
    els.canvas.addEventListener('pointercancel', onPointerUp);

    els.stage.addEventListener('pointermove', onStagePointerMove);
    els.stage.addEventListener('pointerleave', () => sendPointer(0, 0, false));

    $$('#reactionButtons .reaction').forEach((button) => {
      button.addEventListener('click', () => send({ type: 'reaction', emoji: button.dataset.emoji }));
    });

    els.clipSend.addEventListener('click', async () => {
      if (!navigator.clipboard || !navigator.clipboard.readText) {
        toast('This browser cannot read the clipboard. Paste into the chat instead.');
        return;
      }
      try {
        const text = await navigator.clipboard.readText();
        if (!text) { toast('Your clipboard is empty'); return; }
        send({ type: 'clipboard', text });
        toast('Clipboard sent to the session');
      } catch (error) {
        mediaError(error, 'Reading the clipboard');
      }
    });

    els.clipboardCopy.addEventListener('click', async () => {
      try {
        await navigator.clipboard.writeText(state.clipboardText);
        toast('Copied');
      } catch (error) {
        window.prompt('Copy this text', state.clipboardText);
      }
      els.clipboardToast.hidden = true;
    });
    els.clipboardClose.addEventListener('click', () => { els.clipboardToast.hidden = true; });

    els.chatForm.addEventListener('submit', (event) => {
      event.preventDefault();
      const text = els.chatInput.value.trim();
      if (!text) return;
      send({ type: 'chat', text });
      els.chatInput.value = '';
    });

    els.controlRequestBtn.addEventListener('click', () => send({ type: 'control_request' }));

    els.policyEveryone.addEventListener('change', () => {
      send({ type: 'edit_policy', everyone: els.policyEveryone.checked });
    });

    els.peopleList.addEventListener('click', (event) => {
      const button = event.target.closest('button[data-action]');
      if (!button) return;
      const id = Number(button.dataset.id);
      const action = button.dataset.action;
      if (action === 'present') send({ type: 'set_presenter', target: id });
      if (action === 'stop-presenting') send({ type: 'set_presenter', target: null });
      if (action === 'grant') send({ type: 'control_response', to: id, granted: true });
      if (action === 'revoke') send({ type: 'control_revoke', target: id });
      if (action === 'request-control') send({ type: 'control_request' });
      if (action === 'request-present') send({ type: 'present_request' });
    });

    els.codeCopy.addEventListener('click', async () => {
      try {
        await navigator.clipboard.writeText(els.codeArea.value);
        toast('Code copied');
      } catch (error) {
        window.prompt('Copy this code', els.codeArea.value);
      }
    });

    els.codeDownload.addEventListener('click', () => {
      const blob = new Blob([els.codeArea.value], { type: 'text/plain;charset=utf-8' });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement('a');
      anchor.href = url;
      anchor.download = 'triple-scratchpad.txt';
      anchor.click();
      setTimeout(() => URL.revokeObjectURL(url), 2000);
    });

    els.codeOpen.addEventListener('click', () => {
      if (!state.canEdit) { toast('View only — ask the host for control first.'); return; }
      els.codeFile.value = '';
      els.codeFile.click();
    });

    els.codeFile.addEventListener('change', async () => {
      const file = els.codeFile.files && els.codeFile.files[0];
      if (!file) return;
      if (file.size > 512 * 1024) { toast('That file is larger than 512 KB.'); return; }
      const text = await file.text();
      editor.replaceAll(text);
      toast(`Loaded ${file.name}`);
    });

    editor.init(els.codeArea);
  }

  /* ------------------------------------------------------------ connection */

  function connect(name) {
    if (state.ws) {
      try { state.ws.close(); } catch (error) { /* ignore */ }
    }
    const scheme = location.protocol === 'https:' ? 'wss' : 'ws';
    const param = state.role === 'host'
      ? `key=${encodeURIComponent(state.credential)}`
      : `token=${encodeURIComponent(state.credential)}`;
    const ws = new WebSocket(`${scheme}://${location.host}/ws?${param}`);
    state.ws = ws;

    ws.addEventListener('open', () => {
      setConnection('online');
      send({ type: 'hello', name });
    });

    ws.addEventListener('message', (event) => {
      let message;
      try { message = JSON.parse(event.data); } catch (error) { return; }
      handleMessage(message);
    });

    ws.addEventListener('close', () => {
      setConnection('offline');
      closeAllPeers();
      clearRemoteMedia();
      if (state.leaving) return;
      clearTimeout(state.reconnectTimer);
      state.reconnectTimer = setTimeout(() => connect(state.me.name || 'Guest'), 1500);
    });

    ws.addEventListener('error', () => setConnection('offline'));
  }

  function setConnection(status) {
    els.connStatus.dataset.state = status;
    els.connStatus.querySelector('.conn-text').textContent =
      status === 'online' ? 'connected' : status === 'offline' ? 'reconnecting…' : 'connecting';
  }

  function handleMessage(message) {
    switch (message.type) {
      case 'welcome': return onWelcome(message);
      case 'join': return onJoin(message);
      case 'leave': return onLeave(message);
      case 'presenter': return onPresenter(message);
      case 'chat': return onChat(message);
      case 'reaction': return addReaction(message.emoji, message.name);
      case 'annotation': return onAnnotation(message.stroke);
      case 'annotation_clear': return clearAnnotations();
      case 'clipboard': return onClipboard(message);
      case 'signal': return onSignal(message.from, message.data);
      case 'control_request': return onControlRequest(message);
      case 'present_request': return onPresentRequest(message);
      case 'control_result': return onControlResult(message);
      case 'control_state': return onControlState(message);
      case 'edit_policy': return onEditPolicy(message);
      case 'ot_ack': return editor.ack(message.rev);
      case 'ot': return editor.remote(message.rev, message.ops);
      case 'ot_resync': return editor.reset(message.text, message.rev);
      case 'pointer': return showCursor(message.from, message.name, message.x, message.y, message.active);
      case 'error': return toast(message.message || 'Server error', { timeout: 7000 });
      case 'pong': return;
      default: return;
    }
  }

  function onWelcome(message) {
    state.me.id = message.self.id;
    state.me.name = message.self.name;
    state.me.role = message.self.role;
    state.participants = new Map(message.participants.map((p) => [p.id, p]));
    state.presenter = message.presenter;
    state.policyEveryone = !!message.policy_everyone;
    state.canEdit = !!message.can_edit;
    state.inviteUrl = message.invite_url || '';
    state.chat = Array.isArray(message.chat) ? message.chat : [];
    state.strokes = Array.isArray(message.strokes) ? message.strokes : [];
    els.policyEveryone.checked = state.policyEveryone;
    els.inviteBtn.hidden = state.me.role !== 'host' || !state.inviteUrl;
    els.hostControls.hidden = state.me.role !== 'host';
    els.selfLabel.textContent = state.me.name;
    editor.reset(message.doc.text, message.doc.rev);
    renderChat();
    drawStrokes();
    renderAll();
    for (const participant of state.participants.values()) {
      if (participant.id !== state.me.id) ensurePeer(participant.id);
    }
    announceAllStreams();
    if (state.me.role === 'host') {
      toast('Session ready. Copy the invite link and share it with your pair.', { timeout: 7000 });
    }
  }

  function onJoin(message) {
    const participant = message.participant;
    if (!participant || participant.id === state.me.id) return;
    state.participants.set(participant.id, participant);
    ensurePeer(participant.id);
    toast(`${participant.name} joined`, { timeout: 2600 });
    renderAll();
  }

  function onLeave(message) {
    const participant = state.participants.get(message.id);
    state.participants.delete(message.id);
    removePeer(message.id);
    if (participant) toast(`${participant.name} left`, { timeout: 2600 });
    renderAll();
  }

  function onPresenter(message) {
    const previous = state.presenter;
    state.presenter = message.id;
    if (previous === state.me.id && state.presenter !== state.me.id) {
      stopScreenShare();
      toast('You are no longer presenting');
    }
    if (state.presenter === state.me.id && previous !== state.me.id) {
      toast('You are presenting — click “Share screen”.', { timeout: 7000 });
    }
    renderAll();
  }

  function onChat(message) {
    state.chat.push(message);
    if (state.chat.length > 200) state.chat.shift();
    renderChat();
  }

  function onAnnotation(stroke) {
    if (!stroke) return;
    state.strokes.push(stroke);
    drawStrokes();
  }

  function onClipboard(message) {
    state.clipboardText = message.text;
    els.clipboardFrom.textContent = `${message.name || 'Someone'} sent their clipboard`;
    els.clipboardPreview.textContent = message.text.slice(0, 500);
    els.clipboardToast.hidden = false;
  }

  function onControlRequest(message) {
    if (state.me.role !== 'host') return;
    toast(`${message.name} is asking for control`, {
      actions: [
        { label: 'Grant', onClick: () => send({ type: 'control_response', to: message.from, granted: true }) },
        { label: 'Deny', onClick: () => send({ type: 'control_response', to: message.from, granted: false }) },
      ],
    });
  }

  function onPresentRequest(message) {
    if (state.me.role !== 'host') return;
    toast(`${message.name} would like to present`, {
      actions: [
        { label: 'Make presenter', onClick: () => send({ type: 'set_presenter', target: message.from }) },
      ],
    });
  }

  function onControlResult(message) {
    toast(message.granted ? 'The host granted you control' : 'The host declined control', { timeout: 5000 });
  }

  function onControlState(message) {
    const participant = state.participants.get(message.id);
    if (participant) {
      participant.has_control = !!message.granted;
      participant.can_edit = participant.role === 'host' || state.policyEveryone || !!message.granted;
    }
    if (message.id === state.me.id) {
      state.canEdit = state.me.role === 'host' || state.policyEveryone || !!message.granted;
      applyEditability();
    }
    renderPeople();
  }

  function onEditPolicy(message) {
    state.policyEveryone = !!message.everyone;
    els.policyEveryone.checked = state.policyEveryone;
    for (const participant of state.participants.values()) {
      participant.can_edit = participant.role === 'host' || state.policyEveryone || !!participant.has_control;
    }
    const me = state.participants.get(state.me.id);
    if (me) {
      state.canEdit = !!me.can_edit;
      applyEditability();
    }
    renderPeople();
    toast(state.policyEveryone ? 'Everyone can now edit the scratchpad' : 'Only the host and granted guests can edit');
  }

  /* --------------------------------------------------------------- editor */

  const editor = {
    ta: null,
    lastValue: '',
    rev: 0,
    outstanding: [],
    applying: false,

    init(ta) {
      this.ta = ta;
      this.lastValue = ta.value;
      ta.addEventListener('input', () => this.onInput());
    },

    reset(text, rev) {
      this.applying = true;
      this.ta.value = text;
      this.lastValue = text;
      this.rev = rev;
      this.outstanding = [];
      this.applying = false;
      this.status();
    },

    status() {
      const pending = this.outstanding.length;
      els.codeStatus.textContent = pending ? `rev ${this.rev} · ${pending} pending` : `rev ${this.rev} · synced`;
    },

    onInput() {
      if (this.applying) return;
      const oldValue = this.lastValue;
      const newValue = this.ta.value;
      if (oldValue === newValue) return;
      if (!state.canEdit) {
        this.applying = true;
        this.ta.value = oldValue;
        this.lastValue = oldValue;
        this.applying = false;
        toast('View only — ask the host for control.');
        return;
      }
      const ops = diff(oldValue, newValue);
      this.lastValue = newValue;
      if (!ops.length) return;
      const baseRev = this.rev + this.outstanding.length;
      this.outstanding.push(ops);
      send({ type: 'ot', rev: baseRev, ops });
      this.status();
    },

    replaceAll(text) {
      if (!state.canEdit) { toast('View only — ask the host for control first.'); return; }
      const current = this.ta.value;
      if (current === text) return;
      const ops = diff(current, text);
      const baseRev = this.rev + this.outstanding.length;
      this.outstanding.push(ops);
      this.applying = true;
      this.ta.value = text;
      this.lastValue = text;
      this.applying = false;
      send({ type: 'ot', rev: baseRev, ops });
      this.status();
    },

    ack(rev) {
      this.rev = rev;
      this.outstanding.shift();
      this.status();
    },

    remote(rev, ops) {
      let remoteOps = ops;
      for (let i = 0; i < this.outstanding.length; i += 1) {
        const [mine, theirs] = transform(this.outstanding[i], remoteOps);
        this.outstanding[i] = mine;
        remoteOps = theirs;
      }
      if (!this.outstanding.length) {
        if (rev !== this.rev + 1) {
          send({ type: 'resync' });
          return;
        }
        this.rev = rev;
      }
      const ta = this.ta;
      const before = ta.value;
      const after = applyOps(before, remoteOps);
      if (after === before) { this.status(); return; }
      const start = ta.selectionStart;
      const end = ta.selectionEnd;
      const focused = document.activeElement === ta;
      this.applying = true;
      ta.value = after;
      this.lastValue = after;
      if (focused) {
        try {
          const mappedStart = mapPosition(start, remoteOps);
          const mappedEnd = mapPosition(end, remoteOps);
          ta.setSelectionRange(mappedStart, mappedEnd);
        } catch (error) { /* ignore */ }
      }
      this.applying = false;
      this.status();
    },
  };

  function diff(oldStr, newStr) {
    if (oldStr === newStr) return [];
    let prefix = 0;
    const max = Math.min(oldStr.length, newStr.length);
    while (prefix < max && oldStr.charCodeAt(prefix) === newStr.charCodeAt(prefix)) prefix += 1;
    let suffix = 0;
    while (
      suffix < oldStr.length - prefix &&
      suffix < newStr.length - prefix &&
      oldStr.charCodeAt(oldStr.length - 1 - suffix) === newStr.charCodeAt(newStr.length - 1 - suffix)
    ) suffix += 1;
    const ops = [];
    if (prefix) ops.push({ retain: prefix });
    const deleted = oldStr.length - prefix - suffix;
    if (deleted) ops.push({ delete: deleted });
    const inserted = newStr.slice(prefix, newStr.length - suffix);
    if (inserted) ops.push({ insert: inserted });
    if (suffix) ops.push({ retain: suffix });
    return ops;
  }

  function applyOps(text, ops) {
    let pos = 0;
    let out = '';
    for (const op of ops) {
      if ('retain' in op) {
        out += text.slice(pos, pos + op.retain);
        pos += op.retain;
      } else if ('delete' in op) {
        pos += op.delete;
      } else if ('insert' in op) {
        out += op.insert;
      }
    }
    return out + text.slice(pos);
  }

  function mapPosition(position, ops) {
    let oldPos = 0;
    let newPos = 0;
    for (const op of ops) {
      if ('retain' in op) {
        if (position <= oldPos + op.retain) return newPos + (position - oldPos);
        oldPos += op.retain;
        newPos += op.retain;
      } else if ('insert' in op) {
        newPos += op.insert.length;
      } else if ('delete' in op) {
        if (position <= oldPos + op.delete) return newPos;
        oldPos += op.delete;
      }
    }
    return newPos + (position - oldPos);
  }

  function atoms(ops) {
    const out = [];
    for (const op of ops) {
      if ('retain' in op) {
        for (let i = 0; i < op.retain; i += 1) out.push({ t: 'r' });
      } else if ('delete' in op) {
        for (let i = 0; i < op.delete; i += 1) out.push({ t: 'd' });
      } else if ('insert' in op && op.insert) {
        out.push({ t: 'i', s: op.insert });
      }
    }
    return out;
  }

  function mergeAtoms(atomsList) {
    const merged = [];
    for (const atom of atomsList) {
      const last = merged[merged.length - 1];
      if (last && last.t === atom.t && atom.t !== 'i') last.n += 1;
      else merged.push({ ...atom, n: atom.t === 'i' ? atom.s.length : 1 });
    }
    return merged.map((atom) => {
      if (atom.t === 'i') return { insert: atom.s };
      if (atom.t === 'r') return { retain: atom.n };
      return { delete: atom.n };
    });
  }

  function transform(a, b) {
    const A = atoms(a);
    const B = atoms(b);
    const aOut = [];
    const bOut = [];
    let i = 0;
    let j = 0;
    while (i < A.length || j < B.length) {
      const x = A[i];
      const y = B[j];
      if (x && x.t === 'i') {
        bOut.push({ t: 'r' });
        aOut.push(x);
        i += 1;
        continue;
      }
      if (y && y.t === 'i') {
        aOut.push({ t: 'r' });
        bOut.push(y);
        j += 1;
        continue;
      }
      if (x && y) {
        if (x.t === 'r' && y.t === 'r') {
          aOut.push({ t: 'r' });
          bOut.push({ t: 'r' });
        } else if (x.t === 'd' && y.t === 'd') {
          /* both delete */
        } else if (x.t === 'd') {
          aOut.push({ t: 'd' });
        } else {
          bOut.push({ t: 'd' });
        }
        i += 1;
        j += 1;
      } else if (x) {
        aOut.push(x);
        i += 1;
      } else if (y) {
        bOut.push(y);
        j += 1;
      }
    }
    return [mergeAtoms(aOut), mergeAtoms(bOut)];
  }

  /* -------------------------------------------------------------- webrtc */

  function ensurePeer(id) {
    if (id === state.me.id) return null;
    const existing = state.pcs.get(id);
    if (existing) return existing;

    const polite = state.me.id > id;
    const pc = new RTCPeerConnection({ iceServers: state.iceServers });
    const entry = {
      id,
      pc,
      polite,
      makingOffer: false,
      ignoreOffer: false,
      settingRemoteAnswer: false,
      pendingIce: [],
      dc: null,
    };
    state.pcs.set(id, entry);

    pc.addEventListener('negotiationneeded', async () => {
      try {
        entry.makingOffer = true;
        await pc.setLocalDescription();
        sendSignal(id, {
          kind: 'description',
          type: pc.localDescription.type,
          sdp: pc.localDescription.sdp,
        });
      } catch (error) {
        console.warn('negotiation failed', error);
      } finally {
        entry.makingOffer = false;
      }
    });

    pc.addEventListener('icecandidate', (event) => {
      if (event.candidate) {
        sendSignal(id, { kind: 'ice', candidate: event.candidate.toJSON() });
      }
    });

    pc.addEventListener('track', (event) => attachRemoteTrack(id, event));
    pc.addEventListener('connectionstatechange', () => {
      if (pc.connectionState === 'failed') {
        try { pc.restartIce(); } catch (error) { /* ignore */ }
      }
    });

    pc.addEventListener('datachannel', (event) => setupDataChannel(entry, event.channel));
    if (state.me.id < id) {
      setupDataChannel(entry, pc.createDataChannel('triple', { ordered: true }));
    }

    syncPeerTracks(pc);
    return entry;
  }

  function setupDataChannel(entry, channel) {
    entry.dc = channel;
    channel.addEventListener('open', () => announceAllStreams());
    channel.addEventListener('message', (event) => {
      let message;
      try { message = JSON.parse(event.data); } catch (error) { return; }
      if (message.t === 'stream' && message.id) {
        state.streamKinds.set(message.id, message.kind || 'cam');
        renderMedia();
      } else if (message.t === 'pointer') {
        showCursor(entry.id, state.participants.get(entry.id)?.name || 'Guest', message.x, message.y, message.active);
      }
    });
    channel.addEventListener('close', () => { if (entry.dc === channel) entry.dc = null; });
  }

  function dcSend(entry, message) {
    if (entry.dc && entry.dc.readyState === 'open') {
      try { entry.dc.send(JSON.stringify(message)); return true; } catch (error) { return false; }
    }
    return false;
  }

  function announceAllStreams() {
    for (const [kind, stream] of Object.entries(state.local)) {
      if (!stream) continue;
      state.streamKinds.set(stream.id, kind);
      for (const entry of state.pcs.values()) dcSend(entry, { t: 'stream', id: stream.id, kind });
    }
  }

  function sendSignal(to, data) {
    send({ type: 'signal', to, data });
  }

  async function onSignal(from, data) {
    if (!data) return;
    const entry = ensurePeer(from);
    if (!entry) return;
    const pc = entry.pc;

    if (data.kind === 'ice') {
      const candidate = data.candidate;
      if (!candidate) return;
      if (pc.remoteDescription && pc.remoteDescription.type) {
        try { await pc.addIceCandidate(candidate); } catch (error) { if (!entry.ignoreOffer) console.warn('ice', error); }
      } else {
        entry.pendingIce.push(candidate);
      }
      return;
    }

    if (data.kind !== 'description' || !data.sdp) return;
    const description = { type: data.type, sdp: data.sdp };
    const readyForOffer = !entry.makingOffer && (pc.signalingState === 'stable' || entry.settingRemoteAnswer);
    const offerCollision = description.type === 'offer' && !readyForOffer;
    entry.ignoreOffer = !entry.polite && offerCollision;
    if (entry.ignoreOffer) return;

    entry.settingRemoteAnswer = description.type === 'answer';
    try {
      await pc.setRemoteDescription(description);
    } catch (error) {
      console.warn('setRemoteDescription', error);
      entry.settingRemoteAnswer = false;
      return;
    }
    entry.settingRemoteAnswer = false;
    for (const candidate of entry.pendingIce.splice(0)) {
      try { await pc.addIceCandidate(candidate); } catch (error) { /* ignore */ }
    }
    if (description.type === 'offer') {
      syncPeerTracks(pc);
      await pc.setLocalDescription();
      sendSignal(from, {
        kind: 'description',
        type: pc.localDescription.type,
        sdp: pc.localDescription.sdp,
      });
    }
  }

  function streamForTrack(track) {
    for (const stream of Object.values(state.local)) {
      if (stream && stream.getTracks().includes(track)) return stream;
    }
    return new MediaStream([track]);
  }

  function syncPeerTracks(pc) {
    const wanted = [];
    if (state.local.screen) wanted.push(...state.local.screen.getTracks());
    if (state.local.cam) wanted.push(...state.local.cam.getVideoTracks());
    if (state.local.mic) wanted.push(...state.local.mic.getAudioTracks());
    const senders = pc.getSenders();
    const current = senders.map((sender) => sender.track).filter(Boolean);
    for (const track of wanted) {
      if (!current.includes(track)) pc.addTrack(track, streamForTrack(track));
    }
    for (const sender of senders) {
      if (sender.track && !wanted.includes(sender.track)) {
        try { pc.removeTrack(sender); } catch (error) { /* ignore */ }
      }
    }
  }

  function syncAllTracks() {
    for (const entry of state.pcs.values()) syncPeerTracks(entry.pc);
  }

  function closeAllPeers() {
    for (const entry of state.pcs.values()) {
      try { entry.pc.close(); } catch (error) { /* ignore */ }
    }
    state.pcs.clear();
  }

  function removePeer(id) {
    const entry = state.pcs.get(id);
    if (entry) {
      try { entry.pc.close(); } catch (error) { /* ignore */ }
      state.pcs.delete(id);
    }
    for (const [streamId, record] of state.remoteStreams.entries()) {
      if (record.peerId === id) {
        state.remoteStreams.delete(streamId);
        removeAudio(streamId);
      }
    }
    renderMedia();
  }

  function attachRemoteTrack(peerId, event) {
    const stream = event.streams[0] || new MediaStream([event.track]);
    const kind = state.streamKinds.get(stream.id) || guessKind(peerId, event.track);
    state.remoteStreams.set(stream.id, { peerId, stream, kind });
    event.track.addEventListener('unmute', renderMedia);
    event.track.addEventListener('mute', renderMedia);
    renderMedia();
  }

  function guessKind(peerId, track) {
    if (track.kind === 'audio') return 'mic';
    if (peerId === state.presenter) return 'screen';
    return 'cam';
  }

  /* --------------------------------------------------------------- media */

  async function toggleScreenShare() {
    if (state.local.screen) { stopScreenShare(); return; }
    if (!navigator.mediaDevices || !navigator.mediaDevices.getDisplayMedia) {
      toast('This browser does not support screen capture.', { timeout: 7000 });
      return;
    }
    if (state.me.role === 'host') {
      if (state.presenter !== state.me.id) {
        state.presenter = state.me.id;
        send({ type: 'set_presenter', target: state.me.id });
      }
    } else if (state.presenter !== state.me.id) {
      toast('Only the presenter shares the screen. Ask the host to make you presenter.', { timeout: 7000 });
      return;
    }
    let stream;
    try {
      stream = await navigator.mediaDevices.getDisplayMedia({
        video: { frameRate: { ideal: 30 } },
        audio: true,
      });
    } catch (error) {
      if (error && error.name === 'NotAllowedError') { mediaError(error, 'Screen capture'); return; }
      try {
        stream = await navigator.mediaDevices.getDisplayMedia({ video: true });
      } catch (secondError) {
        mediaError(secondError, 'Screen capture');
        return;
      }
    }
    state.local.screen = stream;
    const videoTrack = stream.getVideoTracks()[0];
    if (videoTrack) videoTrack.addEventListener('ended', stopScreenShare);
    announceAllStreams();
    syncAllTracks();
    renderToolbar();
    renderMedia();
    toast('Sharing your screen');
  }

  function stopScreenShare() {
    if (!state.local.screen) return;
    for (const track of state.local.screen.getTracks()) {
      track.removeEventListener('ended', stopScreenShare);
      track.stop();
    }
    state.local.screen = null;
    syncAllTracks();
    renderToolbar();
    renderMedia();
  }

  async function toggleCamera() {
    if (state.local.cam) {
      stopLocal('cam');
      return;
    }
    if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
      toast('This browser does not expose a camera.', { timeout: 7000 });
      return;
    }
    try {
      const stream = await navigator.mediaDevices.getUserMedia({
        video: { width: { ideal: 1280 }, height: { ideal: 720 } },
      });
      state.local.cam = stream;
      announceAllStreams();
      syncAllTracks();
      renderToolbar();
      renderMedia();
    } catch (error) {
      mediaError(error, 'Camera');
    }
  }

  async function toggleMic() {
    if (state.local.mic) {
      stopLocal('mic');
      return;
    }
    if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
      toast('This browser does not expose a microphone.', { timeout: 7000 });
      return;
    }
    try {
      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      state.local.mic = stream;
      announceAllStreams();
      syncAllTracks();
      renderToolbar();
    } catch (error) {
      mediaError(error, 'Microphone');
    }
  }

  function stopLocal(kind) {
    const stream = state.local[kind];
    if (!stream) return;
    for (const track of stream.getTracks()) track.stop();
    state.local[kind] = null;
    syncAllTracks();
    renderToolbar();
    renderMedia();
  }

  function stopAllMedia() {
    for (const kind of ['screen', 'cam', 'mic']) stopLocal(kind);
  }

  function clearRemoteMedia() {
    for (const streamId of state.remoteStreams.keys()) removeAudio(streamId);
    state.remoteStreams.clear();
    renderMedia();
  }

  function removeAudio(streamId) {
    const audio = audioEls.get(streamId);
    if (audio) {
      audio.srcObject = null;
      audio.remove();
      audioEls.delete(streamId);
    }
  }

  function ensureAudio(streamId, stream) {
    let audio = audioEls.get(streamId);
    if (!audio) {
      audio = document.createElement('audio');
      audio.autoplay = true;
      audio.playsInline = true;
      audio.className = 'remote-audio';
      document.body.appendChild(audio);
      audioEls.set(streamId, audio);
    }
    if (audio.srcObject !== stream) {
      audio.srcObject = stream;
      audio.play().catch(() => { /* autoplay blocked until interaction */ });
    }
  }

  function stageStream() {
    if (state.presenter == null) return null;
    let cam = null;
    for (const record of state.remoteStreams.values()) {
      if (record.peerId !== state.presenter) continue;
      if (record.kind === 'screen') return record;
      if (record.kind === 'cam') cam = record;
    }
    return cam;
  }

  function renderMedia() {
    for (const [streamId, record] of state.remoteStreams.entries()) {
      if (record.stream.getAudioTracks().length) ensureAudio(streamId, record.stream);
      else removeAudio(streamId);
    }
    for (const streamId of Array.from(audioEls.keys())) {
      if (!state.remoteStreams.has(streamId)) removeAudio(streamId);
    }

    const main = stageStream();
    if (main) {
      if (els.screenVideo.srcObject !== main.stream) {
        els.screenVideo.srcObject = main.stream;
        els.screenVideo.hidden = false;
        playStage();
      }
      els.screenVideo.hidden = false;
      els.stageEmpty.hidden = true;
    } else {
      els.screenVideo.hidden = true;
      els.screenVideo.srcObject = null;
      els.stageEmpty.hidden = false;
      const presenter = state.participants.get(state.presenter);
      if (presenter) {
        els.stageEmptyTitle.textContent = `${presenter.name} is presenting`;
        els.stageEmptyHint.textContent = 'Waiting for their screen or camera…';
      } else {
        els.stageEmptyTitle.textContent = 'No one is presenting';
        els.stageEmptyHint.textContent = state.me.role === 'host'
          ? 'Click “Share screen”, or make someone the presenter from the People tab.'
          : 'The presenter shares their screen and it appears here.';
      }
    }

    if (state.local.cam) {
      if (els.selfVideo.srcObject !== state.local.cam) els.selfVideo.srcObject = state.local.cam;
      els.selfWrap.hidden = false;
    } else {
      els.selfWrap.hidden = true;
      els.selfVideo.srcObject = null;
    }

    renderFilmstrip();
    renderBadges();
  }

  function playStage() {
    const attempt = els.screenVideo.play();
    if (!attempt || !attempt.catch) return;
    attempt.catch(() => {
      toast('The shared screen is ready but your browser blocked autoplay.', {
        timeout: 0,
        actions: [{ label: 'Play', onClick: () => els.screenVideo.play().catch(() => {}) }],
      });
    });
  }

  function renderFilmstrip() {
    const main = stageStream();
    const streams = [];
    for (const record of state.remoteStreams.values()) {
      if (record.kind !== 'cam') continue;
      if (main && main.stream === record.stream) continue;
      if (record.stream.getVideoTracks().length === 0) continue;
      streams.push(record);
    }
    els.filmstrip.innerHTML = '';
    for (const record of streams) {
      const tile = document.createElement('div');
      tile.className = 'film-tile';
      const video = document.createElement('video');
      video.autoplay = true;
      video.playsInline = true;
      video.muted = true;
      video.srcObject = record.stream;
      const label = document.createElement('span');
      label.textContent = state.participants.get(record.peerId)?.name || 'Guest';
      tile.append(video, label);
      els.filmstrip.appendChild(tile);
    }
  }

  function renderBadges() {
    const presenter = state.participants.get(state.presenter);
    els.stageBadges.innerHTML = '';
    if (presenter) {
      const badge = document.createElement('span');
      badge.className = 'badge';
      badge.textContent = `Presented by ${presenter.name}`;
      els.stageBadges.appendChild(badge);
    }
    if (state.local.screen) {
      const badge = document.createElement('span');
      badge.className = 'badge live';
      badge.textContent = 'You are sharing';
      els.stageBadges.appendChild(badge);
    }
  }

  function renderToolbar() {
    els.shareBtn.textContent = state.local.screen ? 'Stop sharing' : 'Share screen';
    els.shareBtn.classList.toggle('active', !!state.local.screen);
    els.camBtn.classList.toggle('active', !!state.local.cam);
    els.micBtn.classList.toggle('active', !!state.local.mic);
    els.camBtn.textContent = state.local.cam ? 'Camera on' : 'Camera';
    els.micBtn.textContent = state.local.mic ? 'Mic on' : 'Mic';
    const canShare = state.me.role === 'host' || state.presenter === state.me.id;
    els.shareBtn.disabled = !canShare && !state.local.screen;
    els.shareBtn.title = canShare ? '' : 'Ask the host to make you presenter';
  }

  /* ---------------------------------------------------------- annotations */

  function resizeCanvas() {
    const rect = els.canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    els.canvas.width = Math.max(1, Math.round(rect.width * dpr));
    els.canvas.height = Math.max(1, Math.round(rect.height * dpr));
    drawStrokes();
  }

  function canvasPoint(event) {
    const rect = els.canvas.getBoundingClientRect();
    return [
      Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width)),
      Math.min(1, Math.max(0, (event.clientY - rect.top) / rect.height)),
    ];
  }

  function onPointerDown(event) {
    if (!state.annotating) return;
    event.preventDefault();
    els.canvas.setPointerCapture(event.pointerId);
    const point = canvasPoint(event);
    state.liveStroke = {
      color: els.penColor.value,
      width: 3.5,
      points: [point],
      last: point,
    };
    drawLiveSegment(point, point);
  }

  function onPointerMove(event) {
    if (!state.liveStroke) return;
    const point = canvasPoint(event);
    state.liveStroke.points.push(point);
    drawLiveSegment(state.liveStroke.last, point);
    state.liveStroke.last = point;
  }

  function onPointerUp() {
    if (!state.liveStroke) return;
    const stroke = state.liveStroke;
    state.liveStroke = null;
    if (stroke.points.length < 2) stroke.points.push(stroke.points[0]);
    const message = {
      type: 'annotation',
      color: stroke.color,
      width: stroke.width,
      points: stroke.points.map(([x, y]) => [round3(x), round3(y)]),
    };
    send(message);
  }

  function round3(value) {
    return Math.round(value * 1000) / 1000;
  }

  function drawLiveSegment(from, to) {
    const ctx = els.canvas.getContext('2d');
    const w = els.canvas.width;
    const h = els.canvas.height;
    ctx.strokeStyle = state.liveStroke ? state.liveStroke.color : '#ff5c5c';
    ctx.lineWidth = (state.liveStroke ? state.liveStroke.width : 3) * (window.devicePixelRatio || 1);
    ctx.lineCap = 'round';
    ctx.lineJoin = 'round';
    ctx.beginPath();
    ctx.moveTo(from[0] * w, from[1] * h);
    ctx.lineTo(to[0] * w, to[1] * h);
    ctx.stroke();
  }

  function drawStrokes() {
    const ctx = els.canvas.getContext('2d');
    ctx.clearRect(0, 0, els.canvas.width, els.canvas.height);
    const w = els.canvas.width;
    const h = els.canvas.height;
    const scale = window.devicePixelRatio || 1;
    for (const stroke of state.strokes) {
      if (!stroke.points || stroke.points.length === 0) continue;
      ctx.strokeStyle = stroke.color || '#ff5c5c';
      ctx.lineWidth = (stroke.width || 3) * scale;
      ctx.lineCap = 'round';
      ctx.lineJoin = 'round';
      ctx.beginPath();
      const [firstX, firstY] = stroke.points[0];
      ctx.moveTo(firstX * w, firstY * h);
      for (let i = 1; i < stroke.points.length; i += 1) {
        const [x, y] = stroke.points[i];
        ctx.lineTo(x * w, y * h);
      }
      ctx.stroke();
    }
  }

  function clearAnnotations() {
    state.strokes = [];
    drawStrokes();
  }

  /* ------------------------------------------------------ reactions/cursor */

  function addReaction(emoji, name) {
    const el = document.createElement('div');
    el.className = 'reaction-float';
    el.textContent = emoji;
    el.style.left = `${10 + Math.random() * 80}%`;
    if (name) el.title = name;
    els.reactionLayer.appendChild(el);
    el.addEventListener('animationend', () => el.remove());
    setTimeout(() => el.remove(), 4000);
  }

  function sendPointer(x, y, active) {
    const now = performance.now();
    if (active && now - state.lastPointer < 60) return;
    state.lastPointer = now;
    let sent = false;
    for (const entry of state.pcs.values()) {
      if (dcSend(entry, { t: 'pointer', x: round3(x), y: round3(y), active })) sent = true;
    }
    if (!sent) send({ type: 'pointer', x, y, active });
  }

  function onStagePointerMove(event) {
    if (state.annotating) return;
    const rect = els.stage.getBoundingClientRect();
    const x = Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width));
    const y = Math.min(1, Math.max(0, (event.clientY - rect.top) / rect.height));
    sendPointer(x, y, true);
  }

  function showCursor(id, name, x, y, active) {
    let record = cursors.get(id);
    if (!active) {
      if (record) { record.el.remove(); clearTimeout(record.timer); cursors.delete(id); }
      return;
    }
    if (!record) {
      const el = document.createElement('div');
      el.className = 'cursor';
      const dot = document.createElement('span');
      dot.className = 'cursor-dot';
      const label = document.createElement('span');
      label.className = 'cursor-label';
      el.append(dot, label);
      els.cursorLayer.appendChild(el);
      record = { el, label, timer: null };
      cursors.set(id, record);
    }
    record.label.textContent = name || 'Guest';
    record.el.style.left = `${x * 100}%`;
    record.el.style.top = `${y * 100}%`;
    clearTimeout(record.timer);
    record.timer = setTimeout(() => {
      record.el.remove();
      cursors.delete(id);
    }, 3500);
  }

  /* ---------------------------------------------------------------- people */

  function renderAll() {
    renderPeopleStrip();
    renderPeople();
    renderMedia();
    renderToolbar();
    applyEditability();
    renderBadges();
  }

  function renderPeopleStrip() {
    els.peopleStrip.innerHTML = '';
    const people = Array.from(state.participants.values()).sort((a, b) => {
      if (a.role !== b.role) return a.role === 'host' ? -1 : 1;
      return a.id - b.id;
    });
    for (const person of people) {
      const chip = document.createElement('span');
      chip.className = 'person-chip';
      if (person.id === state.presenter) chip.classList.add('presenting');
      chip.textContent = initials(person.name);
      chip.title = `${person.name}${person.role === 'host' ? ' (host)' : ''}${person.id === state.presenter ? ' — presenting' : ''}`;
      els.peopleStrip.appendChild(chip);
    }
  }

  function renderPeople() {
    els.peopleList.innerHTML = '';
    const people = Array.from(state.participants.values()).sort((a, b) => {
      if (a.role !== b.role) return a.role === 'host' ? -1 : 1;
      return a.id - b.id;
    });
    for (const person of people) {
      const row = document.createElement('div');
      row.className = 'person-row';
      if (person.id === state.presenter) row.classList.add('presenting');

      const avatar = document.createElement('span');
      avatar.className = 'avatar';
      avatar.textContent = initials(person.name);

      const info = document.createElement('div');
      info.className = 'person-info';
      const name = document.createElement('span');
      name.className = 'person-name';
      name.textContent = person.name + (person.id === state.me.id ? ' (you)' : '');
      const meta = document.createElement('span');
      meta.className = 'person-meta';
      const bits = [];
      if (person.role === 'host') bits.push('host');
      if (person.id === state.presenter) bits.push('presenting');
      if (person.can_edit) bits.push(person.role === 'host' ? 'editor' : 'can edit');
      meta.textContent = bits.join(' · ') || 'guest';
      info.append(name, meta);

      const actions = document.createElement('div');
      actions.className = 'person-actions';

      if (state.me.role === 'host') {
        if (person.role !== 'host') {
          const presentBtn = document.createElement('button');
          presentBtn.className = 'btn tiny ghost';
          presentBtn.textContent = person.id === state.presenter ? 'Stop presenting' : 'Present';
          presentBtn.dataset.action = person.id === state.presenter ? 'stop-presenting' : 'present';
          presentBtn.dataset.id = String(person.id);
          actions.appendChild(presentBtn);

          const controlBtn = document.createElement('button');
          controlBtn.className = 'btn tiny ghost';
          controlBtn.textContent = person.has_control ? 'Revoke' : 'Give control';
          controlBtn.dataset.action = person.has_control ? 'revoke' : 'grant';
          controlBtn.dataset.id = String(person.id);
          actions.appendChild(controlBtn);
        }
      } else if (person.id === state.me.id) {
        if (!state.canEdit) {
          const requestBtn = document.createElement('button');
          requestBtn.className = 'btn tiny';
          requestBtn.textContent = 'Request control';
          requestBtn.dataset.action = 'request-control';
          requestBtn.dataset.id = String(person.id);
          actions.appendChild(requestBtn);
        }
        if (state.presenter !== state.me.id) {
          const askBtn = document.createElement('button');
          askBtn.className = 'btn tiny ghost';
          askBtn.textContent = 'Ask to present';
          askBtn.dataset.action = 'request-present';
          askBtn.dataset.id = String(person.id);
          actions.appendChild(askBtn);
        }
      }

      row.append(avatar, info, actions);
      els.peopleList.appendChild(row);
    }

    const empty = document.createElement('p');
    empty.className = 'muted center';
    if (!people.length) {
      empty.textContent = 'Nobody here yet.';
      els.peopleList.appendChild(empty);
    }
  }

  function applyEditability() {
    els.codeArea.readOnly = !state.canEdit;
    els.codeArea.classList.toggle('readonly', !state.canEdit);
    els.controlBanner.hidden = state.canEdit;
    els.controlBannerText.textContent = 'View only — ask the host for control';
    els.controlRequestBtn.hidden = state.canEdit || state.me.role === 'host';
    editor.status();
  }

  /* ------------------------------------------------------------------ chat */

  function renderChat() {
    els.chatLog.innerHTML = '';
    for (const message of state.chat) {
      if (!message || message.type !== 'chat') continue;
      const row = document.createElement('div');
      row.className = 'chat-msg';
      if (message.from === state.me.id) row.classList.add('mine');
      const head = document.createElement('div');
      head.className = 'chat-head';
      head.textContent = `${message.name || 'Guest'} · ${formatTime(message.ts)}`;
      const body = document.createElement('div');
      body.className = 'chat-body';
      body.textContent = message.text;
      row.append(head, body);
      els.chatLog.appendChild(row);
    }
    els.chatLog.scrollTop = els.chatLog.scrollHeight;
  }

  function formatTime(ts) {
    if (!ts) return '';
    try {
      return new Date(ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    } catch (error) {
      return '';
    }
  }

  /* ----------------------------------------------------------------- init */

  init().catch((error) => {
    console.error(error);
    els.joinError.hidden = false;
    els.joinError.textContent = String(error);
  });
})();
