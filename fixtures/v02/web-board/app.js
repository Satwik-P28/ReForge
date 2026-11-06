(() => {
  const STORAGE = "refboard";
  const status = document.getElementById("status");
  const list = document.getElementById("tasks");
  const count = document.getElementById("count");
  const title = document.getElementById("title");

  function fresh() {
    return { tasks: [], filter: "all", next: 1 };
  }

  function load() {
    try {
      const raw = localStorage.getItem(STORAGE);
      return raw ? JSON.parse(raw) : fresh();
    } catch {
      return fresh();
    }
  }

  function save(state) {
    localStorage.setItem(STORAGE, JSON.stringify(state));
  }

  function render() {
    const state = load();
    const visible = state.tasks.filter(
      (task) => state.filter === "all" || !task.done,
    );
    list.replaceChildren();
    for (const task of visible) {
      const item = document.createElement("li");
      const label = document.createElement("span");
      label.textContent = `${task.title}${task.done ? " (done)" : ""}`;
      const done = document.createElement("button");
      done.type = "button";
      done.textContent = "Done";
      done.setAttribute("aria-label", `Done ${task.title}`);
      done.addEventListener("click", () => {
        const current = load();
        const target = current.tasks.find((entry) => entry.id === task.id);
        if (!target) {
          status.textContent = "No such task";
          return;
        }
        if (target.done) {
          status.textContent = "Already done";
          return;
        }
        target.done = true;
        save(current);
        status.textContent = "Task completed";
        render();
      });
      const remove = document.createElement("button");
      remove.type = "button";
      remove.textContent = "Delete";
      remove.setAttribute("aria-label", `Delete ${task.title}`);
      remove.addEventListener("click", () => {
        const current = load();
        const target = current.tasks.find((entry) => entry.id === task.id);
        if (!target) {
          status.textContent = "No such task";
          return;
        }
        if (!target.done) {
          status.textContent = "Complete the task before deleting";
          return;
        }
        current.tasks = current.tasks.filter((entry) => entry.id !== task.id);
        save(current);
        status.textContent = "Task deleted";
        render();
      });
      item.append(label, done, remove);
      list.append(item);
    }
    count.textContent = `${state.tasks.length} task(s), filter ${state.filter}`;
  }

  document.getElementById("add-form").addEventListener("submit", (event) => {
    event.preventDefault();
    const value = title.value.trim();
    if (!value) {
      status.textContent = "Title required";
      return;
    }
    const state = load();
    if (state.tasks.some((task) => task.title === value)) {
      status.textContent = "Duplicate title";
      return;
    }
    state.tasks.push({ id: `t${state.next}`, title: value, done: false });
    state.next += 1;
    state.filter = "all";
    save(state);
    title.value = "";
    status.textContent = "Task added";
    render();
  });

  document.getElementById("filter-open").addEventListener("click", () => {
    const state = load();
    state.filter = "open";
    save(state);
    status.textContent = "Showing open";
    render();
  });

  document.getElementById("filter-all").addEventListener("click", () => {
    const state = load();
    state.filter = "all";
    save(state);
    status.textContent = "Showing all";
    render();
  });

  document.getElementById("clear-done").addEventListener("click", () => {
    const state = load();
    if (!state.tasks.some((task) => task.done)) {
      status.textContent = "Nothing completed";
      return;
    }
    state.tasks = state.tasks.filter((task) => !task.done);
    state.filter = "all";
    save(state);
    status.textContent = "Cleared completed";
    render();
  });

  render();
})();
