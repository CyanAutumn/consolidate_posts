export type AssetKind = "image" | "video" | "text" | "model" | "file";
export interface Asset {
  id: string;
  path: string;
  name: string;
  kind: AssetKind;
  size: number;
  modifiedMs: number;
  sha256?: string | null;
}
export interface Draft {
  title: string;
  body: string;
  assetIds: string[];
}
export interface Driver {
  id: string;
  name: string;
  category: string;
  description: string;
  status: string;
  kinds: AssetKind[];
  acceptsTitle: boolean;
  acceptsBody: boolean;
  maxFiles: number | null;
  maxFileBytes: number | null;
}
export type JobStatus =
  | "scheduled"
  | "waiting_archive"
  | "queued"
  | "running"
  | "cancelling"
  | "succeeded"
  | "failed"
  | "cancelled"
  | "interrupted";
export interface Job {
  id: string;
  batchId: string;
  driverId: string;
  status: JobStatus;
  completedBytes: number;
  totalBytes: number;
  createdAt: number;
  attempt: number;
  retryOf: string | null;
  error: { code: string; message: string } | null;
  receipt: {
    location: string;
    files: {
      assetId: string;
      fileName: string;
      size: number;
      sha256: string;
    }[];
  } | null;
  publication: { title: string; body: string; assets: Asset[] };
  kind: "legacy" | "archive" | "upload";
  targetId: string | null;
  archiveId: string | null;
  dependsOn: string | null;
  planId: string | null;
  scheduledAt: number | null;
}
export interface UploadTarget {
  id: string;
  driverId: string;
  name: string;
  createdAt: number;
}
export interface ArchiveRecord {
  id: string;
  sourceJobId: string;
  createdAt: number;
  publication: Job["publication"];
  receipt: NonNullable<Job["receipt"]>;
}
export type PlanStatus = "active" | "paused" | "cancelled";
export interface ReplayPlan {
  id: string;
  targetId: string;
  createdAt: number;
  startAt: number;
  intervalMinutes: number;
  status: PlanStatus;
  nextReleaseAt: number;
  archiveIds: string[];
  force: boolean;
}
export interface HistoryState {
  archives: ArchiveRecord[];
  targets: UploadTarget[];
  plans: ReplayPlan[];
}
export interface ReplayRequest {
  archiveIds: string[];
  targetId: string;
  startAt: number;
  intervalMinutes: number;
  force: boolean;
}
export interface ReplayPreview {
  eligibleCount: number;
  skippedCount: number;
  items: {
    archiveId: string;
    title: string;
    eligible: boolean;
    reason: string | null;
    scheduledAt: number | null;
  }[];
}
export interface ReplayResult {
  plan: ReplayPlan;
  preview: ReplayPreview;
}
export interface Snapshot {
  platformAccounts?: PlatformAccount[];
  workspace: WorkspaceProfile;
  workspaces: WorkspaceProfile[];
  pluginDir: string;
  pluginReports: PluginReport[];
  drivers: Driver[];
  draft: Draft;
  assets: Asset[];
  jobs: Job[];
  dataDir: string;
  archiveDir: string;
  concurrency: number;
  history: HistoryState;
}
export interface PluginReport {
  folder: string;
  id: string | null;
  name: string | null;
  version: string | null;
  state: string;
  message: string;
}
export type ContentRating = "unspecified" | "all_ages" | "r18";
export interface WorkspaceConfig {
  name: string;
  contentRating: ContentRating;
  enabledDriverIds: string[];
}
export interface WorkspaceProfile extends WorkspaceConfig {
  id: string;
  createdAt: number;
  selectedTargetIds: string[];
}
export interface Validation {
  driverId: string;
  eligible: boolean;
  issues: string[];
}
export interface ImportReport {
  assets: Asset[];
  errors: { path: string; message: string }[];
}
export interface Client {
  platformAccounts(): Promise<PlatformAccount[]>;
  savePixivSettings(targetId: string, settings: PixivSettings): Promise<PixivSettings>;
  loginXiaohongshu(targetId: string): Promise<void>;
  loginX(targetId: string): Promise<void>;
  pixivSettings(targetId: string): Promise<PixivSettings>;
  loginPixiv(targetId: string, settings: PixivSettings): Promise<void>;
  desktop: boolean;
  forWorkspace(id: string): Client;
  createWorkspace(config: WorkspaceConfig): Promise<Snapshot>;
  switchWorkspace(id: string): Promise<Snapshot>;
  updateWorkspace(config: WorkspaceConfig): Promise<Snapshot>;
  selectTargets(ids: string[]): Promise<void>;
  bootstrap(): Promise<Snapshot>;
  importPaths(paths: string[]): Promise<ImportReport>;
  chooseFiles(): Promise<string[]>;
  saveDraft(draft: Draft): Promise<void>;
  preflight(draft: Draft): Promise<Validation[]>;
  enqueue(draft: Draft, targetIds: string[]): Promise<Job[]>;
  jobs(): Promise<Job[]>;
  cancel(id: string): Promise<void>;
  retry(id: string): Promise<Job[]>;
  history(): Promise<HistoryState>;
  createTarget(driverId: string, name: string): Promise<UploadTarget>;
  previewReplay(request: ReplayRequest): Promise<ReplayPreview>;
  scheduleReplay(request: ReplayRequest): Promise<ReplayResult>;
  setPlanStatus(id: string, status: PlanStatus): Promise<void>;
  onJobsChanged(callback: () => void): Promise<() => void>;
  onDrop(
    callback: (paths: string[]) => void,
    hover: (active: boolean) => void,
  ): Promise<() => void>;
}
export interface PixivSettings {
  userId: string; tags: string[]; aiType: string; visibility: string;
  rating: string | null; original: boolean; allowTagEdit: boolean; allowComment: boolean;
}
export interface PlatformAccount { driverId: string; targetId: string; connected: boolean; name: string }
