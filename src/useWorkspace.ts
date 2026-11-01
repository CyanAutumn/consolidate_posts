import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { errorMessage } from "./lib/api";
import type {
  Asset,
  Client,
  Draft,
  HistoryState,
  Job,
  Snapshot,
  Validation,
  WorkspaceConfig,
} from "./types";

const emptyDraft: Draft = { title: "", body: "", assetIds: [] };
export function useWorkspace(rootClient: Client) {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const workspaceId = snapshot?.workspace.id;
  const client = useMemo(
    () => (workspaceId ? rootClient.forWorkspace(workspaceId) : rootClient),
    [rootClient, workspaceId],
  );
  const activeId = useRef(workspaceId);
  activeId.current = workspaceId;
  const [selected, setSelected] = useState<string[]>([]);
  const [savedSelection, setSavedSelection] = useState("[]");
  const [draft, setDraft] = useState<Draft>(emptyDraft);
  const [assets, setAssets] = useState<Asset[]>([]);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [history, setHistory] = useState<HistoryState>({
    archives: [],
    targets: [],
    plans: [],
  });
  const [busy, setBusy] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [savedKey, setSavedKey] = useState(JSON.stringify(emptyDraft));
  const [validation, setValidation] = useState<Validation[]>([]);
  const lock = useRef(false);
  const refreshVersion = useRef(0);
  const refresh = useCallback(async () => {
    const scope = workspaceId;
    const version = ++refreshVersion.current;
    const [next, nextHistory, platformAccounts] = await Promise.all([
      client.jobs(),
      client.history(),
      client.platformAccounts(),
    ]);
    if (version === refreshVersion.current && scope === activeId.current) {
      setJobs(next);
      setHistory(nextHistory);
      setSnapshot(prev => prev && JSON.stringify(prev.platformAccounts ?? []) !== JSON.stringify(platformAccounts) ? { ...prev, platformAccounts } : prev);
    }
  }, [client, workspaceId]);

  const applySnapshot = useCallback((state: Snapshot) => {
    ++refreshVersion.current;
    activeId.current = state.workspace.id;
    setSnapshot(state);
    setDraft(state.draft);
    setAssets(state.assets);
    setJobs(state.jobs);
    setHistory(state.history);
    setSelected(state.workspace.selectedTargetIds);
    setSavedSelection(JSON.stringify(state.workspace.selectedTargetIds));
    setSavedKey(JSON.stringify(state.draft));
    setValidation([]);
    setError("");
    setNotice("");
    setDragging(false);
  }, []);

  useEffect(() => {
    let disposed = false;
    rootClient
      .bootstrap()
      .then((state) => {
        if (disposed) return;
        applySnapshot(state);
      })
      .catch((error) => {
        if (!disposed) setError(errorMessage(error));
      });
    return () => {
      disposed = true;
    };
  }, [rootClient, applySnapshot]);

  useEffect(() => {
    if (!client.desktop || !snapshot) return;
    let disposed = false;
    let unlisten = () => {};
    const update = () => {
      if (!disposed)
        void refresh().catch((error) => {
          if (!disposed && workspaceId === activeId.current)
            setError(errorMessage(error));
        });
    };
    client
      .onJobsChanged(update)
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      })
      .catch((error) => {
        if (!disposed) setError(errorMessage(error));
      });
    const interval = setInterval(() => {
      if (!document.hidden) update();
    }, 2000);
    update();
    return () => {
      disposed = true;
      unlisten();
      clearInterval(interval);
    };
  }, [client, snapshot, refresh, workspaceId]);

  const run = useCallback(async (action: () => Promise<void>) => {
    if (lock.current) return;
    lock.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      await action();
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }, []);

  const importPaths = useCallback(
    async (paths: string[]) => {
      if (!paths.length) return;
      const report = await client.importPaths(paths);
      setAssets((previous) => {
        const merged = new Map(previous.map((asset) => [asset.id, asset]));
        report.assets.forEach((asset) => merged.set(asset.id, asset));
        return [...merged.values()];
      });
      setDraft((previous) => ({
        ...previous,
        assetIds: [
          ...new Set([
            ...previous.assetIds,
            ...report.assets.map((asset) => asset.id),
          ]),
        ],
      }));
      setValidation([]);
      if (report.errors.length)
        setError(
          report.errors
            .map((item) => `${item.path}：${item.message}`)
            .join("\n"),
        );
      if (report.assets.length)
        setNotice(`已导入 ${report.assets.length} 个文件`);
    },
    [client],
  );
  const dropHandler = useRef<(paths: string[]) => void>(() => {});
  dropHandler.current = (paths) => {
    if (lock.current) {
      setError("正在处理上一项操作，请稍后重新拖入文件");
      return;
    }
    void run(() => importPaths(paths));
  };
  useEffect(() => {
    let disposed = false;
    let unlisten = () => {};
    client
      .onDrop((paths) => dropHandler.current(paths), setDragging)
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      })
      .catch((error) => setError(errorMessage(error)));
    return () => {
      disposed = true;
      unlisten();
    };
  }, [client]);

  const updateDraft = (next: Draft) => {
    setDraft(next);
    setValidation([]);
    setNotice("");
  };
  const chooseFiles = () =>
    run(async () => {
      await importPaths(await client.chooseFiles());
    });
  const save = () =>
    run(async () => {
      await saveCurrent();
      setNotice("草稿已保存到本机");
    });
  const saveCurrent = async () => {
    await client.saveDraft(draft);
    await client.selectTargets(selected);
    setSavedKey(JSON.stringify(draft));
    setSavedSelection(JSON.stringify(selected));
  };
  const switchWorkspace = (id: string) =>
    run(async () => {
      if (!snapshot || id === workspaceId) return;
      await saveCurrent();
      applySnapshot(await rootClient.switchWorkspace(id));
    });
  const createWorkspace = (config: WorkspaceConfig) =>
    run(async () => {
      await saveCurrent();
      applySnapshot(await rootClient.createWorkspace(config));
    });
  const updateWorkspace = (config: WorkspaceConfig) =>
    run(async () => {
      await saveCurrent();
      applySnapshot(await client.updateWorkspace(config));
      setNotice("工作区设置已保存");
    });
  const check = () =>
    run(async () => {
      setValidation(await client.preflight(draft));
      setNotice("发布检查完成");
    });
  const publish = (selected: string[], done: () => void) =>
    run(async () => {
      await saveCurrent();
      const created = await client.enqueue(draft, selected);
      setNotice(`已创建 ${created.length} 个任务，先本地留档，再执行上传`);
      done();
      await refresh();
    });
  const cancel = (id: string) =>
    run(async () => {
      await client.cancel(id);
      await refresh();
    });
  const retry = (id: string) =>
    run(async () => {
      await client.retry(id);
      setNotice("已创建重试任务");
      await refresh();
    });
  const draftAssets = draft.assetIds
    .map((id) => assets.find((asset) => asset.id === id))
    .filter((asset): asset is Asset => Boolean(asset));
  return {
    client,
    selected,
    setSelected,
    switchWorkspace,
    createWorkspace,
    updateWorkspace,
    snapshot,
    draft,
    updateDraft,
    assets: draftAssets,
    jobs,
    history,
    refresh,
    busy,
    dragging,
    error,
    notice,
    setError,
    saved:
      savedKey === JSON.stringify(draft) &&
      savedSelection === JSON.stringify(selected),
    validation,
    chooseFiles,
    save,
    check,
    publish,
    cancel,
    retry,
  };
}
