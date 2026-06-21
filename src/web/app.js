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

  const dashboard = document.querySelector("[data-guild-id]");
  if (!dashboard || !window.EventSource) return;

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
      const body = new FormData(form);
      if (submitter?.name) body.set(submitter.name, submitter.value);
      if (submitter) submitter.disabled = true;
      try {
        const response = await fetch(form.action, { method: "POST", body });
        if (response.ok) {
          window.location.assign(response.url);
          return;
        }
        const documentText = await response.text();
        const parsed = new DOMParser().parseFromString(documentText, "text/html");
        const message = parsed.querySelector(".error-page p:not(.eyebrow)")?.textContent || "Action failed.";
        showToast(message, true);
      } catch {
        showToast("Dashboard tidak bisa menghubungi bot.", true);
      } finally {
        if (submitter) submitter.disabled = false;
      }
    });
  });

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
