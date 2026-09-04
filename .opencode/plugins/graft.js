// GraftPlugin — mantém o grafo de contexto `graft/` em sync após edições.
//
// Substitui os hooks legados `.agents/settings.json` + `.agents/helpers/*.cjs`
// (formato Claude Code, com paths quebrados). Roda `graft build` com debounce,
// fire-and-forget: falha silenciosa se o `graft` não estiver instalado.
export const GraftPlugin = async ({ $, directory }) => {
  let timer = null;

  async function scheduleRebuild() {
    if (timer) clearTimeout(timer);
    timer = setTimeout(async () => {
      timer = null;
      try {
        await $`graft build`.cwd(directory).quiet().nothrow();
      } catch {
        // graft ausente ou repo sem índice — no-op
      }
    }, 10000);
  }

  return {
    "tool.execute.after": async (input) => {
      const tool = input?.tool ?? "";
      if (tool === "edit" || tool === "write") await scheduleRebuild();
    },
  };
};
