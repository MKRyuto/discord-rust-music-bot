(() => {
  const showToast = (message, isError = false) => {
    document.querySelector("[data-toast]")?.remove();
    const toast = document.createElement("div");
    toast.className = `toast${isError ? " error" : ""}`;
    toast.dataset.toast = "";
    toast.setAttribute("role", isError ? "alert" : "status");
    const text = document.createElement("span");
    text.textContent = message;
    const close = document.createElement("button");
    close.type = "button";
    close.textContent = "Close";
    close.setAttribute("aria-label", "Dismiss");
    close.addEventListener("click", () => toast.remove());
    toast.append(text, close);
    document.body.append(toast);
    window.setTimeout(() => toast.remove(), 5000);
  };

  const initialToast = document.querySelector("[data-toast]");
  document.querySelector("[data-dismiss-toast]")?.addEventListener("click", () => initialToast?.remove());
  if (initialToast) window.setTimeout(() => initialToast.remove(), 5000);

  document.querySelectorAll("time[data-unix]").forEach((element) => {
    const date = new Date(Number(element.dataset.unix) * 1000);
    if (!Number.isNaN(date.valueOf())) element.textContent = date.toLocaleString();
  });

  const commandSearch = document.querySelector("[data-command-search]");
  const commandRows = [...document.querySelectorAll("[data-command]")];
  const commandFilters = [...document.querySelectorAll("[data-command-filter]")];
  let activeCategory = "all";
  const filterCommands = () => {
    const query = commandSearch?.value.trim().toLowerCase() || "";
    let visible = 0;
    commandRows.forEach((row) => {
      const categoryMatch = activeCategory === "all" || row.dataset.category === activeCategory;
      const searchMatch = !query || row.textContent.toLowerCase().includes(query);
      row.hidden = !(categoryMatch && searchMatch);
      if (!row.hidden) visible += 1;
    });
    const count = document.querySelector("[data-command-count]");
    const empty = document.querySelector("[data-command-empty]");
    if (count) count.textContent = visible;
    if (empty) empty.hidden = visible !== 0;
  };
  commandSearch?.addEventListener("input", filterCommands);
  commandFilters.forEach((button) => button.addEventListener("click", () => {
    activeCategory = button.dataset.commandFilter;
    commandFilters.forEach((item) => item.classList.toggle("active", item === button));
    filterCommands();
  }));

  const loading = document.createElement("div");
  loading.className = "action-loading";
  loading.hidden = true;
  loading.innerHTML = '<span class="loading-indicator"><span class="loading-spinner" aria-hidden="true"></span><span class="loading-check" aria-hidden="true">&#10003;</span></span><strong data-loading-label>Working...</strong>';
  document.body.append(loading);
  const showLoading = (label) => {
    loading.classList.remove("success");
    loading.querySelector("[data-loading-label]").textContent = label;
    loading.hidden = false;
  };
  const showLoadingSuccess = (label = "Success") => {
    loading.classList.add("success");
    loading.querySelector("[data-loading-label]").textContent = label;
  };
  const hideLoading = () => { loading.hidden = true; };
  const setButtonBusy = (button, busy) => {
    if (!button) return;
    button.disabled = busy;
    button.classList.toggle("is-loading", busy);
    button.setAttribute("aria-busy", String(busy));
  };

  const requestAction = async (url, body, label, options = {}) => {
    const shouldReload = options.reload !== false;
    showLoading(label);
    let navigating = false;
    try {
      const response = await fetch(url, {
        method: "POST",
        headers: { "Content-Type": "application/x-www-form-urlencoded" },
        body: body.toString(),
        redirect: "manual",
      });
      if (response.type === "opaqueredirect" || response.status === 0 ||
          (response.status >= 300 && response.status < 400) || response.ok) {
        showLoadingSuccess(shouldReload ? "Success. Refreshing..." : "Saved");
        await new Promise((resolve) => window.setTimeout(resolve, shouldReload ? 300 : 180));
        if (shouldReload) {
          navigating = true;
          window.location.reload();
        } else {
          hideLoading();
          showToast(options.successMessage || "Changes saved.");
        }
        return true;
      }
      const documentText = await response.text();
      const parsed = new DOMParser().parseFromString(documentText, "text/html");
      const message = parsed.querySelector(".error-page p:not(.eyebrow)")?.textContent || "Action failed.";
      showToast(message, true);
      return false;
    } catch {
      showToast("Dashboard tidak bisa menghubungi bot.", true);
      return false;
    } finally {
      if (!navigating) hideLoading();
    }
  };

  document.querySelectorAll("form[data-async-form]").forEach((form) => {
    form.addEventListener("submit", async (event) => {
      event.preventDefault();
      const submitter = event.submitter;
      const confirmation = submitter?.dataset.confirm;
      if (confirmation && !window.confirm(confirmation)) return;
      setButtonBusy(submitter, true);
      try {
        const body = new URLSearchParams(new FormData(form));
        if (submitter?.name) body.set(submitter.name, submitter.value);
        await requestAction(
          form.action,
          body,
          submitter?.textContent?.trim() || "Sending...",
        );
      } finally {
        setButtonBusy(submitter, false);
      }
    });
  });

  document.querySelectorAll("a[href]").forEach((link) => {
    link.addEventListener("click", (event) => {
      if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey ||
          event.shiftKey || event.altKey || link.target === "_blank" || link.hasAttribute("download")) return;
      const target = new URL(link.href, window.location.href);
      if (target.origin !== window.location.origin ||
          (target.pathname === window.location.pathname && target.search === window.location.search && target.hash)) return;
      showLoading(link.dataset.loadingLabel || "Opening page...");
    });
  });
  window.addEventListener("pageshow", hideLoading);

  const dashboard = document.querySelector("[data-guild-id]");
  if (!dashboard) return;

  const editorStateKey = `playlist-editor:${window.location.pathname}`;
  const restoreEditorState = () => {
    try {
      const saved = JSON.parse(sessionStorage.getItem(editorStateKey) || "null");
      if (!saved) return;
      document.querySelectorAll("[data-playlist-entry]").forEach((entry) => {
        if (saved.openPlaylists.includes(entry.dataset.playlistName)) {
          const details = entry.querySelector("details");
          if (details) details.open = true;
        }
      });
      window.requestAnimationFrame(() => window.scrollTo(0, saved.scrollY));
    } catch {
      // Ignore stale browser state.
    } finally {
      sessionStorage.removeItem(editorStateKey);
    }
  };
  const saveEditorState = (form) => {
    if (!form.closest("[data-playlist-entry]") || form.action.endsWith("/playlists/delete")) return false;
    const entry = form.closest("[data-playlist-entry]");
    const currentName = entry.dataset.playlistName;
    const renamedTo = form.action.endsWith("/playlists/rename")
      ? form.querySelector('input[name="new_name"]')?.value.trim()
      : null;
    const openPlaylists = [...document.querySelectorAll("[data-playlist-entry]")]
      .filter((entry) => entry.querySelector("details")?.open)
      .map((item) => item.dataset.playlistName === currentName && renamedTo ? renamedTo : item.dataset.playlistName);
    sessionStorage.setItem(editorStateKey, JSON.stringify({ openPlaylists, scrollY: window.scrollY }));
    return true;
  };
  restoreEditorState();

  const playlistDialog = document.querySelector("[data-playlist-dialog]");
  document.querySelector("[data-open-playlist-dialog]")?.addEventListener("click", () => playlistDialog?.showModal());
  document.querySelector("[data-close-playlist-dialog]")?.addEventListener("click", () => playlistDialog?.close());
  document.querySelectorAll("[data-playlist-mode]").forEach((button) => button.addEventListener("click", () => {
    const mode = button.dataset.playlistMode;
    document.querySelectorAll("[data-playlist-mode]").forEach((item) => item.classList.toggle("active", item === button));
    document.querySelectorAll("[data-playlist-panel]").forEach((panel) => { panel.hidden = panel.dataset.playlistPanel !== mode; });
  }));

  const playlistSearch = document.querySelector("[data-playlist-search]");
  const filterPlaylists = () => {
    const query = playlistSearch?.value.trim().toLowerCase() || "";
    let visible = 0;
    document.querySelectorAll("[data-playlist-entry]").forEach((entry) => {
      entry.hidden = Boolean(query) && !entry.textContent.toLowerCase().includes(query);
      if (!entry.hidden) visible += 1;
    });
    const empty = document.querySelector("[data-playlist-empty]");
    if (empty) empty.hidden = visible !== 0;
  };
  playlistSearch?.addEventListener("input", filterPlaylists);

  document.querySelectorAll("[data-track-search]").forEach((input) => input.addEventListener("input", () => {
    const query = input.value.trim().toLowerCase();
    input.closest("details")?.querySelectorAll("[data-playlist-track]").forEach((row) => {
      row.hidden = Boolean(query) && !row.textContent.toLowerCase().includes(query);
    });
  }));

  dashboard.querySelectorAll("form").forEach((form) => {
    form.addEventListener("submit", async (event) => {
      const submitter = event.submitter;
      const action = submitter?.value;
      const destructive = action === "stop" || action === "clear" ||
        form.action.endsWith("/playlists/delete") ||
        (action === "remove" && form.action.endsWith("/playlists/track"));
      if (destructive && !window.confirm("Continue with this action?")) {
        event.preventDefault();
        return;
      }
      event.preventDefault();
      const body = new URLSearchParams(new FormData(form));
      if (submitter?.name) body.set(submitter.name, submitter.value);
      const inlineTrackAction = form.action.endsWith("/playlists/track") &&
        ["up", "down", "remove"].includes(action);
      const editorStateSaved = inlineTrackAction ? false : saveEditorState(form);
      setButtonBusy(submitter, true);
      try {
        const succeeded = await requestAction(
          form.action,
          body,
          submitter?.textContent?.trim() || "Saving changes...",
          inlineTrackAction ? { reload: false, successMessage: "Playlist updated." } : {},
        );
        if (!succeeded && editorStateSaved) sessionStorage.removeItem(editorStateKey);
        if (succeeded && inlineTrackAction) {
          const item = form.closest("[data-playlist-track]");
          const list = item?.parentElement;
          if (!item || !list) return;
          if (action === "up") item.previousElementSibling?.before(item);
          if (action === "down") item.nextElementSibling?.after(item);
          if (action === "remove") item.remove();
          refreshTrackPositions(list);
          const entry = list.closest("[data-playlist-entry]");
          const count = list.querySelectorAll("[data-playlist-track]").length;
          const countLabel = entry?.querySelector(".playlist-heading > div:first-child span");
          if (countLabel) countLabel.textContent = `${count} tracks`;
          if (count === 0) list.innerHTML = '<li class="empty-row">Playlist is empty. Add the first track above.</li>';
        }
      } finally {
        setButtonBusy(submitter, false);
      }
    });
  });

  let draggedTrack = null;
  const refreshTrackPositions = (list) => {
    const rows = [...list.querySelectorAll("[data-playlist-track]")];
    rows.forEach((item, index) => {
      const position = index + 1;
      item.dataset.position = position;
      const number = item.querySelector(":scope > span");
      const input = item.querySelector('input[name="position"]');
      const up = item.querySelector('button[value="up"]');
      const down = item.querySelector('button[value="down"]');
      if (number) number.textContent = position;
      if (input) input.value = position;
      if (up) up.disabled = index === 0;
      if (down) down.disabled = index === rows.length - 1;
    });
  };
  document.querySelectorAll("[data-playlist-track]").forEach((row) => {
    row.addEventListener("dragstart", () => {
      draggedTrack = row;
      row.classList.add("dragging");
    });
    row.addEventListener("dragend", () => {
      row.classList.remove("dragging");
      document.querySelectorAll(".playlist-tracks > li.drop-target").forEach((item) => item.classList.remove("drop-target"));
      draggedTrack = null;
    });
    row.addEventListener("dragover", (event) => {
      if (draggedTrack && draggedTrack.parentElement === row.parentElement) {
        event.preventDefault();
        if (draggedTrack !== row) row.classList.add("drop-target");
      }
    });
    row.addEventListener("dragleave", () => {
      row.classList.remove("drop-target");
    });
    row.addEventListener("drop", async (event) => {
      event.preventDefault();
      row.classList.remove("drop-target");
      if (!draggedTrack || draggedTrack.parentElement !== row.parentElement || draggedTrack === row) return;
      const source = draggedTrack;
      const target = row;
      const list = source.parentElement;
      const fromPosition = Number(source.dataset.position);
      const toPosition = Number(target.dataset.position);
      const body = new URLSearchParams({
        csrf: source.dataset.csrf,
        name: source.dataset.playlistName,
        position: source.dataset.position,
        to_position: target.dataset.position,
        action: "move",
      });
      const succeeded = await requestAction(source.dataset.actionUrl, body, "Reordering playlist...", {
        reload: false,
        successMessage: "Playlist order updated.",
      });
      if (!succeeded) return;
      if (fromPosition < toPosition) target.after(source);
      else target.before(source);
      refreshTrackPositions(list);
    });
  });

  if (!window.EventSource) return;
  const events = new EventSource(`/dashboard/${dashboard.dataset.guildId}/events`);
  events.addEventListener("player", (message) => {
    const state = JSON.parse(message.data);
    const now = document.querySelector("[data-now-playing]");
    const status = document.querySelector("[data-player-status]");
    const count = document.querySelector("[data-queue-count]");
    if (now) now.textContent = state.now_playing;
    if (status) status.textContent = state.status;
    if (count) count.textContent = state.queue.length;

    const volume = document.querySelector('.player-controls input[name="volume"]');
    if (volume && document.activeElement !== volume) volume.value = state.volume;
    const loop = document.querySelector('.player-controls button[value="loop"]');
    if (loop) loop.textContent = `Loop: ${state.loop_mode}`;
    const pause = document.querySelector('.player-controls button[value="pause"]');
    if (pause) pause.textContent = state.status === "Paused" ? "Resume" : "Pause";

    const rows = [...document.querySelectorAll("[data-queue-item]")];
    if (rows.length !== state.queue.length) {
      window.location.reload();
      return;
    }
    rows.forEach((row, index) => {
      row.querySelector("span").textContent = state.queue[index].position;
      row.querySelector("strong").textContent = state.queue[index].title;
      row.querySelector("small").textContent = state.queue[index].duration;
    });
  });
})();
