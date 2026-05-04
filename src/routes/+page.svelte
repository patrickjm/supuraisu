<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import {
    Archive,
    Download,
    FolderOpen,
    Grip,
    Heart,
    Play,
    RotateCcw,
    AudioWaveform,
    Library,
    Search,
    ShoppingCart,
    RefreshCw,
    SlidersHorizontal,
    Sparkles,
    Square,
    X,
  } from "lucide-svelte";

  type View = "search" | "library" | "collections" | "liked" | "packs";

  type SpliceEnvironmentStatus = {
    app_installed: boolean;
    helper_installed: boolean;
    cert_exists: boolean;
    key_exists: boolean;
  };

  type OwnedAuthStatus = {
    signed_in: boolean;
    username: string;
    email: string;
    credits: number;
    expires_at?: number | null;
    keychain_consent?: boolean;
    helper_connected?: boolean;
    helper_has_token?: boolean;
    helper_synced?: boolean;
  };

  type AuthShakedownStatus = {
    keychain_consent: boolean;
    owned_signed_in: boolean;
    owned_username: string;
    owned_email: string;
    owned_expires_at?: number | null;
    helper_connected: boolean;
    helper_has_token: boolean;
    helper_auth_type: string;
    helper_username: string;
    helper_email: string;
    helper_synced: boolean;
    sync_attempted: boolean;
    errors: string[];
  };

  type DecoderDiagnostics = {
    checked: boolean;
    status: "loading" | "ready" | "missing" | "incompatible" | "error";
    candidate_count: number;
    compatible_path: string;
    compatible_size: number;
    candidate_paths: string[];
    error: string;
  };

  type DiagnosticsInfo = {
    app_version: string;
    keychain_consent: boolean;
    owned_signed_in: boolean;
    owned_username: string;
    owned_email: string;
    helper_connected: boolean;
    helper_has_token: boolean;
    helper_auth_type: string;
    helper_username: string;
    helper_email: string;
    helper_synced: boolean;
    environment: SpliceEnvironmentStatus & {
      app_path: string;
      helper_path: string;
      user_data_path?: string | null;
      cert_path?: string | null;
      key_path?: string | null;
      grpc_port_start: number;
      grpc_port_end: number;
    };
    errors: string[];
  };

  type HelperProbeResult = {
    connected: boolean;
    port?: number;
    errors: string[];
    session?: { auth_type: string; has_token: boolean; sub_channel: string };
    user?: { username: string; email: string; sounds_status: string; credits: number; sounds_plan: number };
    preferences?: { splice_folder_path: string; sample_import_directories: string[] };
  };

  type WasmCandidate = {
    path: string;
    bytes: number[];
  };

  type SampleSummary = {
    file_hash: string;
    filename: string;
    local_path: string;
    bpm: number;
    key: string;
    sample_type: string;
    genre: string;
    provider_name: string;
    price: number;
    purchased: boolean;
    tags: string[];
    preview_url: string;
    waveform_url: string;
    duration: number;
    pack_uuid: string;
    pack_name: string;
    pack_cover_url: string;
  };

  type PackSummary = {
    uuid: string;
    name: string;
    cover_url: string;
    banner_url: string;
    demo_url: string;
    genre: string;
    provider_name: string;
    permalink: string;
    sample_count?: number | null;
  };

  type SampleSearchResult = { total_hits: number; samples: SampleSummary[]; matching_tags: Record<string, number> };
  type CollectionSummary = {
    uuid: string;
    name: string;
    description: string;
    cover_url: string;
    permalink: string;
    sample_count: number;
    pack_count: number;
    created_by_current_user: boolean;
    creator_username: string;
  };
  type SampleTypeFilter = "" | "loop" | "one_shot";

  type SampleFilters = {
    genres: string[];
    instruments: string[];
    keys: string[];
    sampleType: SampleTypeFilter;
    bpmMin: string;
    bpmMax: string;
    sort: string;
    tags: string[];
  };

  const navItems = [
    { id: "search", label: "Search", icon: Search },
    { id: "library", label: "Library", icon: Library },
    { id: "packs", label: "Packs", icon: Sparkles },
  ] satisfies { id: View; label: string; icon: typeof Search }[];

  let activeView = $state<View>("search");
  let environment = $state<SpliceEnvironmentStatus | null>(null);
  let account = $state<HelperProbeResult | null>(null);
  let ownedAuth = $state<OwnedAuthStatus | null>(null);
  let authLoading = $state(false);
  let keychainConsent = $state(typeof localStorage !== "undefined" && localStorage.getItem("supuraisu.keychainConsent") === "true");
  let searchQuery = $state("");
  let libraryQuery = $state("");
  let onlyPurchased = $state(false);
  let libraryTab = $state<"samples" | "packs">("samples");
  let likedTab = $state<"samples" | "packs">("samples");
  let searchFilters = $state<SampleFilters>({ genres: [], instruments: [], keys: [], sampleType: "", bpmMin: "", bpmMax: "", sort: "relevance", tags: [] });
  let libraryFilters = $state<SampleFilters>({ genres: [], instruments: [], keys: [], sampleType: "", bpmMin: "", bpmMax: "", sort: "recency", tags: [] });
  let packFilters = $state<SampleFilters>({ genres: [], instruments: [], keys: [], sampleType: "", bpmMin: "", bpmMax: "", sort: "relevance", tags: [] });
  let searchResult = $state<SampleSearchResult | null>(null);
  let packs = $state<PackSummary[]>([]);
  let likedPackResults = $state<PackSummary[]>([]);
  let collections = $state<CollectionSummary[]>([]);
  let selectedPack = $state<PackSummary | null>(null);
  let selectedCollection = $state<CollectionSummary | null>(null);
  let loadingAccount = $state(false);
  let searching = $state(false);
  let loadingPacks = $state(false);
  let loadingCollections = $state(false);
  let loadingMore = $state(false);
  let updateAvailable = $state<Update | null>(null);
  let updating = $state(false);
  let downloadingHash = $state<string | null>(null);
  let draggingHash = $state<string | null>(null);
  let preparingHash = $state<string | null>(null);
  let openFilterDropdown = $state<"instruments" | "genres" | "keys" | null>(null);
  let error = $state<string | null>(null);
  let diagnosticsOpen = $state(false);
  let diagnostics = $state<DiagnosticsInfo | null>(null);
  let decoderDiagnostics = $state<DecoderDiagnostics | null>(null);
  let loadingDiagnostics = $state(false);

  let audio: HTMLAudioElement | null = null;
  let currentSample = $state<SampleSummary | null>(null);
  let currentObjectUrl: string | null = null;
  let playbackNonce = 0;
  let playingHash = $state<string | null>(null);
  let volume = $state(0.85);
  let currentTime = $state(0);
  let duration = $state(0);
  let previewFailures = $state<string[]>([]);
  let waveformFailures = $state<string[]>([]);
  let loadingWaveforms = $state<string[]>([]);
  let waveformData = $state<Record<string, number[]>>({});
  let playedHashes = $state<string[]>([]);
  let searchTimer: ReturnType<typeof setTimeout> | null = null;
  let searchNonce = 0;
  let didWarmCache = false;
  const cacheTtlMs = 2 * 60 * 1000;
  const samplesPerPage = 50;
  const sampleResultCache = new Map<string, { at: number; result: SampleSearchResult; pagesLoaded: number }>();
  const collectionResultCache = new Map<string, { at: number; result: SampleSearchResult; pagesLoaded: number }>();
  let packsCachedAt = 0;
  let likedPacksCachedAt = 0;
  let collectionsCachedAt = 0;

  let connected = $derived((account?.connected && account?.session?.has_token) || ownedAuth?.signed_in || false);
  let credits = $derived(ownedAuth?.signed_in ? ownedAuth.credits : account?.user?.credits ?? null);
  let downloadedCount = $derived(searchResult?.samples.filter((sample) => sample.local_path).length ?? 0);
  let hasMoreSamples = $derived(searchResult ? searchResult.samples.length < searchResult.total_hits : false);
  let likedPacks = $derived.by(() => {
    const seen = new Set<string>();
    if (likedPackResults.length) return likedPackResults;
    return (searchResult?.samples ?? [])
      .filter((sample) => sample.pack_uuid && !seen.has(sample.pack_uuid) && seen.add(sample.pack_uuid))
      .map((sample) => ({
        uuid: sample.pack_uuid,
        name: sample.pack_name || "Untitled pack",
        cover_url: sample.pack_cover_url,
        banner_url: "",
        demo_url: "",
        genre: sample.genre,
        provider_name: sample.provider_name,
        permalink: "",
        sample_count: null,
      }) satisfies PackSummary);
  });
  let viewTitle = $derived(navItems.find((item) => item.id === activeView)?.label ?? "Search");

  type PreviewWasm = { instance: WebAssembly.Instance; memory: WebAssembly.Memory };
  let previewWasmPromise: Promise<PreviewWasm> | null = null;

  async function loadOwnedAuth() {
    if (!keychainConsent) {
      ownedAuth = { signed_in: false, username: "", email: "", credits: 0, expires_at: null, keychain_consent: false, helper_connected: false, helper_has_token: false, helper_synced: false };
      return;
    }
    authLoading = true;
    try {
      ownedAuth = await invoke<OwnedAuthStatus>("supuraisu_auth_status", { keychainConsent });
    } catch {
      ownedAuth = { signed_in: false, username: "", email: "", credits: 0, expires_at: null, keychain_consent: keychainConsent, helper_connected: false, helper_has_token: false, helper_synced: false };
    } finally {
      authLoading = false;
    }
  }

  function updateKeychainConsent(consent: boolean) {
    keychainConsent = consent;
    localStorage.setItem("supuraisu.keychainConsent", consent ? "true" : "false");
    if (consent) loadOwnedAuth();
  }

  async function signInWithSplice() {
    if (!keychainConsent) {
      error = "Please approve macOS Keychain storage before signing in.";
      return;
    }
    authLoading = true;
    error = null;
    try {
      await invoke("supuraisu_auth_login", { keychainConsent });
    } catch (e) {
      authLoading = false;
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function inspectDecoderWasm(): Promise<DecoderDiagnostics> {
    const requiredExports = [
      "_ZN15SpliceAssetData11isScrambledEPKcm",
      "_ZN15SpliceAssetData19descrambleAudioDataEPKcm",
      "_ZN15SpliceAssetData4dataEv",
      "_ZN15SpliceAssetData4sizeEv",
    ];
    try {
      const candidates = await invoke<WasmCandidate[]>("splice_decoder_wasm_candidates");
      let lastError = "No candidate exposed the required decoder symbols.";
      for (const candidate of candidates) {
        try {
          const module = await WebAssembly.compile(new Uint8Array(candidate.bytes));
          const exports = WebAssembly.Module.exports(module).map((entry) => entry.name);
          if (requiredExports.every((name) => exports.includes(name))) {
            return {
              checked: true,
              status: "ready",
              candidate_count: candidates.length,
              compatible_path: candidate.path,
              compatible_size: candidate.bytes.length,
              candidate_paths: candidates.map((item) => item.path),
              error: "",
            };
          }
        } catch (e) {
          lastError = e instanceof Error ? e.message : String(e);
        }
      }
      return {
        checked: true,
        status: candidates.length ? "incompatible" : "missing",
        candidate_count: candidates.length,
        compatible_path: "",
        compatible_size: 0,
        candidate_paths: candidates.map((item) => item.path),
        error: lastError,
      };
    } catch (e) {
      return {
        checked: true,
        status: "error",
        candidate_count: 0,
        compatible_path: "",
        compatible_size: 0,
        candidate_paths: [],
        error: e instanceof Error ? e.message : String(e),
      };
    }
  }

  async function openDiagnostics() {
    diagnosticsOpen = true;
    loadingDiagnostics = true;
    decoderDiagnostics = { checked: false, status: "loading", candidate_count: 0, compatible_path: "", compatible_size: 0, candidate_paths: [], error: "" };
    try {
      const [appDiagnostics, decoder] = await Promise.all([
        invoke<DiagnosticsInfo>("supuraisu_diagnostics"),
        inspectDecoderWasm(),
      ]);
      diagnostics = appDiagnostics;
      decoderDiagnostics = decoder;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loadingDiagnostics = false;
    }
  }

  async function checkForUpdates() {
    try {
      updateAvailable = await check();
    } catch {
      updateAvailable = null;
    }
  }

  async function installUpdate() {
    if (!updateAvailable || updating) return;
    updating = true;
    error = null;
    try {
      await updateAvailable.downloadAndInstall();
      await relaunch();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      updating = false;
    }
  }

  async function signOut() {
    authLoading = true;
    error = null;
    try {
      await invoke("supuraisu_auth_logout");
      ownedAuth = { signed_in: false, username: "", email: "", credits: 0, expires_at: null, keychain_consent: keychainConsent, helper_connected: false, helper_has_token: false, helper_synced: false };
      await loadAccount();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      authLoading = false;
    }
  }

  async function loadAccount() {
    loadingAccount = true;
    error = null;
    try {
      environment = await invoke<SpliceEnvironmentStatus>("splice_environment_status");
      account = await invoke<HelperProbeResult>("probe_splice_helper");
      await loadOwnedAuth();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loadingAccount = false;
    }
  }

  function cacheKey(options: { term: string; purchasedOnly: boolean; packUuid: string; liked?: boolean; filters?: SampleFilters }) {
    return JSON.stringify(options);
  }

  function fresh<T>(entry: { at: number; result: T } | undefined) {
    return entry && Date.now() - entry.at < cacheTtlMs;
  }

  function restoreCachedSamples(term: string, purchasedOnly: boolean, packUuid = "", liked = false) {
    const cached = sampleResultCache.get(cacheKey({ term, purchasedOnly, packUuid, liked, filters: activeFilters() }));
    if (fresh(cached)) {
      searchResult = cached!.result;
      searching = false;
      return true;
    }
    return false;
  }

  function activeFilters() {
    if (selectedPack || selectedCollection) return packFilters;
    return activeView === "library" || activeView === "liked" ? libraryFilters : searchFilters;
  }

  function sortLabel(sort: string) {
    return ({ relevance: "Relevance", recency: "Most recent", popularity: "Popular", bpm: "BPM", filename: "Filename" } as Record<string, string>)[sort] ?? sort;
  }

  async function runSearch(options?: { purchasedOnly?: boolean; liked?: boolean; term?: string; packUuid?: string; force?: boolean; page?: number; append?: boolean; apply?: boolean; filters?: SampleFilters }) {
    const term = options?.term ?? (activeView === "library" || activeView === "liked" ? libraryQuery : searchQuery);
    const purchasedOnly = options?.purchasedOnly ?? onlyPurchased;
    const packUuid = options?.packUuid ?? "";
    const page = options?.page ?? 1;
    const append = options?.append ?? false;
    const apply = options?.apply ?? true;
    const filters = options?.filters ?? activeFilters();
    if (!packUuid && !purchasedOnly && !term.trim()) {
      if (apply) searchResult = null;
      return;
    }

    const liked = options?.liked ?? activeView === "liked";
    const key = cacheKey({ term, purchasedOnly, packUuid, liked, filters });
    const cached = sampleResultCache.get(key);
    if (!append && !options?.force && fresh(cached)) {
      if (apply) {
        searchResult = cached!.result;
        searching = false;
      }
      return;
    }

    const nonce = ++searchNonce;
    if (apply) {
      searching = page === 1 && !cached;
      loadingMore = append;
      if (!append && cached) searchResult = cached.result;
      error = null;
    }
    try {
      const result = await invoke<SampleSearchResult>("search_splice_samples", {
        query: term,
        onlyPurchased: purchasedOnly,
        page,
        perPage: samplesPerPage,
        packUuid,
        liked,
        sampleType: filters.sampleType || undefined,
        bpmMin: filters.bpmMin ? Number(filters.bpmMin) : undefined,
        bpmMax: filters.bpmMax ? Number(filters.bpmMax) : undefined,
        sortFn: filters.sort || undefined,
        tags: [...filters.tags, ...filters.instruments, ...filters.genres, ...filters.keys],
      });
      const merged = append && searchResult
        ? {
            total_hits: result.total_hits,
            matching_tags: result.matching_tags,
            samples: [...searchResult.samples, ...result.samples.filter((incoming) => !searchResult?.samples.some((existing) => existing.file_hash === incoming.file_hash))],
          }
        : result;
      sampleResultCache.set(key, { at: Date.now(), result: merged, pagesLoaded: page });
      if (apply && nonce === searchNonce) searchResult = merged;
    } catch (e) {
      if (apply && nonce === searchNonce) error = e instanceof Error ? e.message : String(e);
    } finally {
      if (apply && nonce === searchNonce) {
        searching = false;
        loadingMore = false;
      }
    }
  }

  async function loadCollections(options?: { force?: boolean; background?: boolean; warmSamples?: boolean }) {
    if (!options?.force && collections.length > 0 && Date.now() - collectionsCachedAt < cacheTtlMs) return;
    loadingCollections = !options?.background && collections.length === 0;
    if (!options?.background) error = null;
    try {
      collections = await invoke<CollectionSummary[]>("list_collections", { page: 1, perPage: 100 });
      collectionsCachedAt = Date.now();
      if (options?.warmSamples) warmCollectionSamples();
    } catch (e) {
      if (!options?.background) error = e instanceof Error ? e.message : String(e);
    } finally {
      loadingCollections = false;
    }
  }

  async function loadLikedPacks(options?: { force?: boolean; background?: boolean }) {
    if (!options?.force && likedPackResults.length > 0 && Date.now() - likedPacksCachedAt < cacheTtlMs) return;
    loadingPacks = !options?.background && likedPackResults.length === 0;
    if (!options?.background) error = null;
    try {
      likedPackResults = await invoke<PackSummary[]>("liked_splice_packs", { limit: 500 });
      likedPacksCachedAt = Date.now();
    } catch (e) {
      if (!options?.background) error = e instanceof Error ? e.message : String(e);
    } finally {
      loadingPacks = false;
    }
  }

  async function loadPacks(options?: { force?: boolean; background?: boolean }) {
    if (!options?.force && packs.length > 0 && Date.now() - packsCachedAt < cacheTtlMs) return;
    loadingPacks = !options?.background && packs.length === 0;
    if (!options?.background) error = null;
    try {
      packs = await invoke<PackSummary[]>("explore_splice_packs", { limit: 24 });
      packsCachedAt = Date.now();
    } catch (e) {
      if (!options?.background) error = e instanceof Error ? e.message : String(e);
      try {
        packs = await invoke<PackSummary[]>("list_helper_packs");
        packsCachedAt = Date.now();
      } catch {
        // Keep the GraphQL error visible when this was user-initiated.
      }
    } finally {
      loadingPacks = false;
    }
  }

  async function switchView(view: View) {
    activeView = view;
    error = null;
    selectedPack = null;
    selectedCollection = null;
    if (view === "library") {
      onlyPurchased = true;
      if (libraryTab === "samples") {
        if (!restoreCachedSamples(libraryQuery, true)) searchResult = null;
        await runSearch({ purchasedOnly: true, term: libraryQuery, filters: libraryFilters });
      } else {
        searchResult = null;
        await loadPacks();
      }
    } else if (view === "search") {
      onlyPurchased = false;
      if (!restoreCachedSamples(searchQuery, onlyPurchased)) searchResult = null;
    } else if (view === "liked") {
      onlyPurchased = true;
      if (likedTab === "packs") {
        searchResult = null;
        await loadLikedPacks();
      } else {
        if (!restoreCachedSamples(libraryQuery, true, "", true)) searchResult = null;
        await runSearch({ purchasedOnly: true, liked: true, term: libraryQuery, filters: libraryFilters });
      }
    } else {
      searchResult = null;
      if (view === "packs" && packs.length === 0) await loadPacks();
      if (view === "collections") await loadCollections();
    }
  }

  function collectionCacheKey(collection: CollectionSummary, filters = activeFilters()) {
    return JSON.stringify({ uuid: collection.uuid, filters });
  }

  async function loadCollectionSamples(collection: CollectionSummary, options?: { page?: number; append?: boolean; apply?: boolean; force?: boolean; filters?: SampleFilters }) {
    const page = options?.page ?? 1;
    const apply = options?.apply ?? true;
    const filters = options?.filters ?? activeFilters();
    const cacheKey = collectionCacheKey(collection, filters);
    const cached = collectionResultCache.get(cacheKey);
    if (!options?.append && !options?.force && fresh(cached)) {
      if (apply) searchResult = cached!.result;
      return;
    }
    if (apply) {
      searching = !cached;
      if (cached) searchResult = cached.result;
      error = null;
    }
    try {
      const result = await invoke<SampleSearchResult>("collection_samples", {
        uuid: collection.uuid,
        page,
        perPage: samplesPerPage,
        sampleType: filters.sampleType || undefined,
        bpmMin: filters.bpmMin ? Number(filters.bpmMin) : undefined,
        bpmMax: filters.bpmMax ? Number(filters.bpmMax) : undefined,
        sortFn: filters.sort || undefined,
        tags: [...filters.tags, ...filters.instruments, ...filters.genres, ...filters.keys],
      });
      const merged = options?.append && cached
        ? {
            total_hits: result.total_hits,
            matching_tags: result.matching_tags,
            samples: [...cached.result.samples, ...result.samples.filter((incoming) => !cached.result.samples.some((existing) => existing.file_hash === incoming.file_hash))],
          }
        : result;
      collectionResultCache.set(cacheKey, { at: Date.now(), result: merged, pagesLoaded: page });
      if (apply) searchResult = merged;
    } catch (e) {
      if (apply) error = e instanceof Error ? e.message : String(e);
    } finally {
      if (apply) searching = false;
    }
  }

  async function warmCollectionSamples() {
    for (const collection of collections) {
      await loadCollectionSamples(collection, { apply: false });
    }
  }

  async function openCollection(collection: CollectionSummary) {
    selectedCollection = collection;
    selectedPack = null;
    await loadCollectionSamples(collection);
  }

  async function openPack(pack: PackSummary) {
    selectedPack = pack;
    selectedCollection = null;
    if (!restoreCachedSamples("", false, pack.uuid)) searchResult = null;
    await runSearch({ term: "", packUuid: pack.uuid, purchasedOnly: false, liked: false, filters: packFilters });
  }

  async function openSamplePack(sample: SampleSummary, event?: Event) {
    event?.stopPropagation();
    if (!sample.pack_uuid) return;
    await openPack({
      uuid: sample.pack_uuid,
      name: sample.pack_name || "Untitled pack",
      cover_url: sample.pack_cover_url,
      banner_url: "",
      demo_url: "",
      genre: sample.genre,
      provider_name: sample.provider_name,
      permalink: "",
      sample_count: null,
    });
  }

  async function loadMoreSamples() {
    if (!searchResult || loadingMore || !hasMoreSamples) return;
    const nextPage = Math.floor(searchResult.samples.length / samplesPerPage) + 1;
    if (selectedCollection) {
      loadingMore = true;
      await loadCollectionSamples(selectedCollection, { page: nextPage, append: true, force: true, filters: activeFilters() });
      loadingMore = false;
      return;
    }
    const packUuid = selectedPack?.uuid ?? "";
    const term = packUuid ? "" : activeView === "library" || activeView === "liked" ? libraryQuery : searchQuery;
    const purchasedOnly = packUuid ? false : activeView === "library" || activeView === "liked" ? true : onlyPurchased;
    await runSearch({ term, purchasedOnly, liked: !packUuid && activeView === "liked", packUuid, page: nextPage, append: true, force: true, filters: activeFilters() });
  }

  function updateSampleEverywhere(sample: SampleSummary) {
    if (searchResult) {
      searchResult = {
        ...searchResult,
        samples: searchResult.samples.map((existing) =>
          existing.file_hash === sample.file_hash ? { ...existing, ...sample } : existing,
        ),
      };
    }
    for (const [key, entry] of sampleResultCache) {
      if (entry.result.samples.some((existing) => existing.file_hash === sample.file_hash)) {
        sampleResultCache.set(key, {
          at: entry.at,
          pagesLoaded: entry.pagesLoaded,
          result: {
            ...entry.result,
            samples: entry.result.samples.map((existing) =>
              existing.file_hash === sample.file_hash ? { ...existing, ...sample } : existing,
            ),
          },
        });
      }
    }
  }

  async function ensureLocalSample(sample: SampleSummary, confirmUnpurchased = true) {
    if (sample.local_path) return sample;
    if (!sample.purchased && confirmUnpurchased) {
      const ok = window.confirm(
        `This sample appears unpurchased and may use ${sample.price} Splice credit${sample.price === 1 ? "" : "s"}. Continue?`,
      );
      if (!ok) return null;
    }

    downloadingHash = sample.file_hash;
    error = null;
    try {
      const result = await invoke<{ requested: boolean; sample?: SampleSummary }>("download_splice_sample", {
        fileHash: sample.file_hash,
      });
      const updated = result.sample ? { ...sample, ...result.sample } : sample;
      updateSampleEverywhere(updated);
      await loadAccount();
      return updated;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      return null;
    } finally {
      downloadingHash = null;
    }
  }

  async function downloadSample(sample: SampleSummary) {
    await ensureLocalSample(sample, true);
  }

  async function startFileDrag(event: PointerEvent, sample: SampleSummary) {
    if (!sample.local_path) return;
    event.preventDefault();
    draggingHash = sample.file_hash;
    error = null;
    try {
      await invoke("start_file_drag", { path: sample.local_path });
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      draggingHash = null;
    }
  }

  async function reveal(sample: SampleSummary) {
    if (!sample.local_path) return;
    try {
      await invoke("reveal_in_finder", { path: sample.local_path });
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function loadPreviewWasm() {
    previewWasmPromise ??= (async () => {
      let memory: WebAssembly.Memory;
      const imports = {
        env: {
          __runjs__: () => 0,
          __createClass__: () => 0,
          __createConstructor__: () => 0,
          __createDestructor__: () => 0,
          __createClassConstant__: () => 0,
          __createStaticProperty__: () => 0,
          __createProperty__: () => 0,
          __createStaticMethod__: () => 0,
          __createMethod__: () => 0,
          __createFunction__: () => 0,
          emscripten_notify_memory_growth: () => {},
          _emscripten_get_progname: () => 0,
        },
        wasi_snapshot_preview1: {
          proc_exit: () => {},
          clock_time_get: () => 0,
          environ_sizes_get: (sizes: number, buf: number) => {
            const view = new DataView(memory.buffer);
            view.setUint32(sizes, 0, true);
            view.setUint32(buf, 0, true);
            return 0;
          },
          environ_get: () => 0,
          fd_close: () => 0,
          fd_read: () => 0,
          fd_write: () => 0,
          fd_seek: () => 0,
        },
      };
      const requiredExports = [
        "_ZN15SpliceAssetData11isScrambledEPKcm",
        "_ZN15SpliceAssetData19descrambleAudioDataEPKcm",
        "_ZN15SpliceAssetData4dataEv",
        "_ZN15SpliceAssetData4sizeEv",
      ];
      const candidates = await invoke<WasmCandidate[]>("splice_decoder_wasm_candidates");
      let lastError = "No compatible Splice decoder WASM found.";
      for (const candidate of candidates) {
        try {
          const wasmBytes = new Uint8Array(candidate.bytes);
          const module = await WebAssembly.compile(wasmBytes);
          const exports = WebAssembly.Module.exports(module).map((entry) => entry.name);
          if (!requiredExports.every((name) => exports.includes(name))) continue;
          const instance = await WebAssembly.instantiate(module, imports);
          memory = instance.exports.memory as WebAssembly.Memory;
          (instance.exports.__initialize__ as (() => void) | undefined)?.();
          return { instance, memory };
        } catch (e) {
          lastError = e instanceof Error ? e.message : String(e);
        }
      }
      throw new Error(lastError);
    })();
    return previewWasmPromise;
  }

  async function descramblePreview(bytes: Uint8Array) {
    const { instance, memory } = await loadPreviewWasm();
    const exports = instance.exports as Record<string, CallableFunction>;
    const malloc = exports.malloc;
    const isScrambled = exports["_ZN15SpliceAssetData11isScrambledEPKcm"];
    const descramble = exports["_ZN15SpliceAssetData19descrambleAudioDataEPKcm"];
    const dataPtr = exports["_ZN15SpliceAssetData4dataEv"];
    const dataSize = exports["_ZN15SpliceAssetData4sizeEv"];

    const inputPtr = malloc(bytes.length) as number;
    new Uint8Array(memory.buffer, inputPtr, bytes.length).set(bytes);
    if (!(isScrambled(inputPtr, bytes.length) as number)) return bytes;

    const assetPtr = malloc(32) as number;
    new Uint8Array(memory.buffer, assetPtr, 32).fill(0);
    descramble(assetPtr, inputPtr, bytes.length);

    const outputPtr = dataPtr(assetPtr) as number;
    const outputSize = dataSize(assetPtr) as number;
    return new Uint8Array(memory.buffer, outputPtr, outputSize).slice();
  }

  function audioMimeForPath(path: string) {
    const lower = path.toLowerCase();
    if (lower.endsWith(".wav")) return "audio/wav";
    if (lower.endsWith(".aif") || lower.endsWith(".aiff")) return "audio/aiff";
    if (lower.endsWith(".mp3")) return "audio/mpeg";
    return "application/octet-stream";
  }

  async function previewSource(sample: SampleSummary) {
    if (sample.local_path) {
      const bytes = await invoke<number[]>("read_local_audio_bytes", { path: sample.local_path });
      if (currentObjectUrl) URL.revokeObjectURL(currentObjectUrl);
      currentObjectUrl = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: audioMimeForPath(sample.local_path) }));
      return currentObjectUrl;
    }
    if (!sample.preview_url) return "";
    if (!sample.preview_url.includes("-scrambled/")) return sample.preview_url;

    const bytes = await invoke<number[]>("fetch_preview_bytes", { url: sample.preview_url });
    const descrambled = await descramblePreview(new Uint8Array(bytes));
    if (currentObjectUrl) URL.revokeObjectURL(currentObjectUrl);
    currentObjectUrl = URL.createObjectURL(new Blob([descrambled], { type: "audio/mpeg" }));
    return currentObjectUrl;
  }

  async function play(sample: SampleSummary) {
    if (!sample.local_path && !sample.preview_url) return;
    if (playingHash === sample.file_hash) {
      stopPlayback();
      return;
    }

    const nonce = ++playbackNonce;
    if (!playedHashes.includes(sample.file_hash)) playedHashes = [...playedHashes, sample.file_hash];
    preparingHash = sample.file_hash;
    error = null;

    let source = "";
    try {
      source = await previewSource(sample);
    } catch (e) {
      if (sample.preview_url?.includes("-scrambled/") && !sample.local_path) {
        if (sample.purchased) {
          const localSample = await ensureLocalSample(sample, false);
          if (localSample?.local_path) {
            sample = localSample;
            source = await previewSource(localSample);
          } else {
            if (nonce !== playbackNonce) return;
            if (!previewFailures.includes(sample.file_hash)) previewFailures = [...previewFailures, sample.file_hash];
            error = "Could not decode this scrambled preview or download the local library file.";
            preparingHash = null;
            return;
          }
        } else {
          if (nonce !== playbackNonce) return;
          if (!previewFailures.includes(sample.file_hash)) previewFailures = [...previewFailures, sample.file_hash];
          error = e instanceof Error ? e.message : String(e);
          preparingHash = null;
          return;
        }
      } else if (sample.preview_url && !sample.local_path) {
        source = sample.preview_url;
      } else {
        if (nonce !== playbackNonce) return;
        if (!previewFailures.includes(sample.file_hash)) previewFailures = [...previewFailures, sample.file_hash];
        error = e instanceof Error ? e.message : String(e);
        preparingHash = null;
        return;
      }
    }
    if (nonce !== playbackNonce) return;
    if (!source) {
      preparingHash = null;
      return;
    }

    audio?.pause();
    const nextAudio = new Audio(source);
    audio = nextAudio;
    nextAudio.volume = volume;
    currentSample = sample;
    currentTime = 0;
    duration = sample.duration || 0;
    playingHash = sample.file_hash;
    preparingHash = null;

    nextAudio.ontimeupdate = () => {
      if (nonce !== playbackNonce || audio !== nextAudio) return;
      currentTime = nextAudio.currentTime;
      duration = Number.isFinite(nextAudio.duration) ? nextAudio.duration : duration;
    };
    nextAudio.onloadedmetadata = () => {
      if (nonce !== playbackNonce || audio !== nextAudio) return;
      duration = Number.isFinite(nextAudio.duration) ? nextAudio.duration : duration;
    };
    nextAudio.onended = () => {
      if (nonce !== playbackNonce || audio !== nextAudio) return;
      stopPlayback(false);
    };
    nextAudio.onerror = () => {
      if (nonce !== playbackNonce || audio !== nextAudio) return;
      stopPlayback(false);
      if (!previewFailures.includes(sample.file_hash)) previewFailures = [...previewFailures, sample.file_hash];
      error = sample.local_path
        ? "Could not play the downloaded local file."
        : "Could not play this remote preview. Some Splice preview URLs are scrambled and need a decoder; download/purchased local files will preview correctly.";
    };

    try {
      await nextAudio.play();
    } catch (e) {
      if (nonce !== playbackNonce || audio !== nextAudio) return;
      stopPlayback(false);
      error = e instanceof Error ? e.message : String(e);
    }
  }

  function stopPlayback(resetCurrent = true) {
    playbackNonce += 1;
    audio?.pause();
    if (audio) audio.currentTime = 0;
    playingHash = null;
    preparingHash = null;
    currentTime = 0;
    if (resetCurrent) currentSample = null;
  }

  function resume() {
    if (!audio || !currentSample) return;
    const nonce = ++playbackNonce;
    audio.play().then(() => {
      if (nonce === playbackNonce && currentSample) playingHash = currentSample.file_hash;
    }).catch((e) => {
      if (nonce !== playbackNonce) return;
      error = e instanceof Error ? e.message : String(e);
    });
  }

  function setVolume(value: number) {
    volume = value;
    if (audio) audio.volume = value;
  }

  function formatMeta(sample: SampleSummary) {
    return [sample.sample_type || null, sample.genre || null].filter(Boolean).join(" · ") || "—";
  }

  function formatTime(value: number) {
    if (!value || value < 0) return "--";
    const seconds = value > 100 ? value / 1000 : value;
    const rounded = Math.round(seconds);
    return `${Math.floor(rounded / 60)}:${String(rounded % 60).padStart(2, "0")}`;
  }

  function resampleWaveform(values: number[], bars = 44) {
    if (!values.length) return [];
    return Array.from({ length: bars }, (_, index) => {
      const start = Math.floor((index * values.length) / bars);
      const end = Math.max(start + 1, Math.floor(((index + 1) * values.length) / bars));
      const peak = Math.max(...values.slice(start, end));
      return Math.max(8, Math.min(100, peak * 100));
    });
  }

  async function ensureWaveform(sample: SampleSummary) {
    if (!sample.waveform_url || waveformData[sample.file_hash] || waveformFailures.includes(sample.file_hash) || loadingWaveforms.includes(sample.file_hash)) return;
    loadingWaveforms = [...loadingWaveforms, sample.file_hash];
    try {
      const bytes = await invoke<number[]>("fetch_preview_bytes", { url: sample.waveform_url });
      const json = new TextDecoder().decode(new Uint8Array(bytes));
      const values = JSON.parse(json);
      if (!Array.isArray(values)) throw new Error("Waveform was not an array");
      waveformData = { ...waveformData, [sample.file_hash]: resampleWaveform(values.filter((value) => typeof value === "number")) };
    } catch {
      waveformFailures = [...waveformFailures, sample.file_hash];
    } finally {
      loadingWaveforms = loadingWaveforms.filter((hash) => hash !== sample.file_hash);
    }
  }

  function waveformBars(sample: SampleSummary) {
    const real = waveformData[sample.file_hash];
    if (real?.length) return real;
    let seed = 0;
    for (const char of sample.file_hash || sample.filename) seed = (seed * 31 + char.charCodeAt(0)) >>> 0;
    return Array.from({ length: 44 }, (_, index) => {
      seed = (seed * 1664525 + 1013904223 + index) >>> 0;
      return 18 + (seed % 70);
    });
  }

  function progressFor(sample: SampleSummary) {
    if (playingHash !== sample.file_hash || !duration) return 0;
    return Math.max(0, Math.min(1, currentTime / duration));
  }

  function initials(sample: SampleSummary) {
    return (sample.provider_name || sample.filename || "S").slice(0, 2).toUpperCase();
  }

  function packSubtitle(pack: PackSummary) {
    return [pack.genre || null, pack.sample_count ? `${pack.sample_count.toLocaleString()} samples` : null]
      .filter(Boolean)
      .join(" · ") || "Pack";
  }

  function sampleCover(sample: SampleSummary) {
    return sample.pack_cover_url || selectedPack?.cover_url || "";
  }

  function sampleTagLabel(sample: SampleSummary) {
    return sample.tags.slice(0, 4).join(" · ") || formatMeta(sample);
  }

  function setSampleType(value: SampleTypeFilter) {
    const filters = activeFilters();
    filters.sampleType = value;
    applyFilters(filters);
  }

  function applyFilters(filters = activeFilters()) {
    searchResult = null;
    if (selectedCollection) {
      loadCollectionSamples(selectedCollection, { force: true, filters });
      return;
    }
    runSearch({
      purchasedOnly: selectedPack ? false : activeView === "library" || activeView === "liked" ? true : onlyPurchased,
      liked: selectedPack ? false : activeView === "liked",
      term: selectedPack ? "" : activeView === "library" || activeView === "liked" ? libraryQuery : searchQuery,
      packUuid: selectedPack?.uuid ?? "",
      filters,
      force: true,
    });
  }

  function updateFilter<K extends keyof SampleFilters>(key: K, value: SampleFilters[K]) {
    const filters = activeFilters();
    filters[key] = value;
    applyFilters(filters);
  }

  function closeFilterDropdowns() {
    openFilterDropdown = null;
  }

  function toggleFilterDropdown(name: "instruments" | "genres" | "keys", event: MouseEvent) {
    event.stopPropagation();
    openFilterDropdown = openFilterDropdown === name ? null : name;
  }

  function toggleFilterValue(key: "genres" | "instruments" | "keys", value: string) {
    const filters = activeFilters();
    const current = filters[key];
    filters[key] = current.includes(value) ? current.filter((item) => item !== value) : [...current, value];
    applyFilters(filters);
  }

  function multiLabel(label: string, values: string[]) {
    if (!values.length) return label;
    if (values.length === 1) return values[0];
    return `${label} · ${values.length}`;
  }

  function sortBy(sort: string) {
    updateFilter("sort", sort);
  }

  function addTag(tag: string) {
    const filters = activeFilters();
    if (!filters.tags.includes(tag)) updateFilter("tags", [...filters.tags, tag]);
  }

  function removeTag(tag: string) {
    const filters = activeFilters();
    updateFilter("tags", filters.tags.filter((existing) => existing !== tag));
  }

  const keys = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
  const genres = ["hip hop", "pop", "house", "techno", "trap", "rnb", "rock", "cinematic", "ambient", "percussion"];
  const instruments = ["drums", "bass", "synth", "guitar", "piano", "vocals", "percussion", "fx", "strings"];
  const sorts = ["relevance", "recency", "popularity", "bpm", "filename"];

  $effect(() => {
    loadAccount();
    checkForUpdates();
    const unlisten = listen<OwnedAuthStatus | { Ok?: OwnedAuthStatus; Err?: string }>("supuraisu-auth-complete", async (event) => {
      authLoading = false;
      const payload = event.payload;
      if (payload && "Err" in payload && payload.Err) {
        error = payload.Err;
        return;
      }
      ownedAuth = payload && "Ok" in payload ? payload.Ok! : (payload as OwnedAuthStatus);
      await loadAccount();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  });

  $effect(() => {
    if (connected && !didWarmCache) {
      didWarmCache = true;
      runSearch({ term: "", purchasedOnly: true, apply: false, filters: libraryFilters });
      runSearch({ term: "", purchasedOnly: true, liked: true, apply: false, filters: libraryFilters });
      loadPacks({ background: true });
      loadLikedPacks({ background: true });
      loadCollections({ background: true, warmSamples: true });
    }
  });

  $effect(() => {
    for (const sample of searchResult?.samples.slice(0, 80) ?? []) {
      ensureWaveform(sample);
    }
  });

  $effect(() => {
    if (searchTimer) clearTimeout(searchTimer);
    if (!connected || selectedPack || selectedCollection || (activeView !== "search" && activeView !== "library" && activeView !== "liked")) return;
    const term = activeView === "library" || activeView === "liked" ? libraryQuery : searchQuery;
    const purchased = activeView === "library" || activeView === "liked" ? true : onlyPurchased;
    searchTimer = setTimeout(() => {
      runSearch({ term, purchasedOnly: purchased, liked: activeView === "liked", filters: activeFilters() });
    }, 250);
  });
</script>

{#snippet sidebarItem(label: string, Icon: typeof Search, active: boolean, action: () => void)}
  <li class="w-full">
    <button class="sidebar-nav-item flex w-full justify-start" class:sidebar-active={active} aria-current={active ? "page" : undefined} onclick={action}>
      <Icon size={14} class="w-5" />
      <span class="truncate">{label}</span>
    </button>
  </li>
{/snippet}


<div class="h-screen overflow-hidden bg-base-100 text-base-content" data-theme="cupcake">
  {#if !connected}
    <main class="grid h-screen place-items-center p-6">
      <section class="card w-full max-w-md border border-base-300 bg-base-100 shadow-xl">
        <div class="card-body items-center text-center">
          <div class="mb-1 flex h-16 w-16 items-center justify-center rounded-2xl bg-primary/15 text-4xl">🧁</div>
          <h1 class="text-3xl font-black">Supuraisu</h1>
          <p class="text-sm text-base-content/65">Welcome. Sign in with your Splice account to browse, preview, download, and drag sounds into your DAW.</p>
          <label class="mt-3 flex gap-2 rounded-box border border-base-300 bg-base-200/60 p-3 text-left text-xs text-base-content/65">
            <input class="checkbox checkbox-primary checkbox-xs mt-0.5" type="checkbox" checked={keychainConsent} onchange={(event) => updateKeychainConsent(event.currentTarget.checked)} />
            <span>
              <strong class="block text-base-content">Keep me signed in</strong>
              Supuraisu will store its Splice auth item in macOS Keychain. macOS may ask for permission; choose <b>Always Allow</b> to avoid repeated prompts.
            </span>
          </label>
          <button class="btn btn-primary btn-sm mt-4" onclick={signInWithSplice} disabled={authLoading || loadingAccount || !keychainConsent}>
            {authLoading ? "Waiting for Splice…" : loadingAccount ? "Checking session…" : "Sign in with Splice"}
          </button>
          {#if error}
            <div class="alert alert-error mt-3 py-2 text-left text-xs"><span>{error}</span></div>
          {/if}
        </div>
      </section>
    </main>
  {:else}
  <div class="ml-44 flex h-screen min-w-0 flex-col overflow-hidden">
    <header class="navbar min-h-10 border-b border-base-300 bg-base-100/95 px-3 py-0 backdrop-blur">
      <div class="flex-1 gap-2">
        <div>
          <h1 class="text-base font-black tracking-tight">
            {selectedCollection?.name ?? selectedPack?.name ?? (activeView === "search" ? "Search" : activeView === "library" ? "Library" : viewTitle)}
          </h1>
          <p class="text-xs text-base-content/60">
            {#if searchResult}
              {searchResult.total_hits.toLocaleString()} results · {downloadedCount} local
            {:else if activeView === "search"}
              Find, preview, download, drag.
            {:else if selectedPack || selectedCollection}
              {selectedCollection ? "Collection sounds" : "Pack sounds"}
            {/if}
          </p>
        </div>
      </div>
      <div class="flex items-center gap-2">
        {#if updateAvailable}
          <button class="btn btn-primary btn-xs" onclick={installUpdate} disabled={updating}>
            {updating ? "Updating…" : `Update ${updateAvailable.version}`}
          </button>
        {/if}
      </div>
    </header>


    <main class="min-h-0 flex-1 overflow-y-auto overflow-x-hidden p-2 pb-14">
      {#if error}
        <div class="alert alert-error mb-3 py-2 text-sm">
          <span class="font-bold">Error</span>
          <span>{error}</span>
        </div>
      {/if}

      {#if activeView === "search" || activeView === "library" || activeView === "liked" || selectedPack || selectedCollection}
        {#if selectedCollection}
          <section class="mb-2 overflow-hidden rounded-box border border-base-300 bg-base-200/40">
            <div class="relative flex items-end gap-3 bg-base-200 p-3">
              {#if selectedCollection.cover_url}<img src={selectedCollection.cover_url} alt="" class="h-16 w-16 rounded-box object-cover shadow" />{:else}<div class="flex h-16 w-16 items-center justify-center rounded-box bg-neutral text-neutral-content shadow"><Archive size={24} /></div>{/if}
              <div class="min-w-0 pb-1">
                <button class="btn btn-ghost btn-xs -ml-2 mb-1" onclick={() => { selectedCollection = null; searchResult = null; switchView("collections"); }}>← Collections</button>
                <h2 class="truncate text-lg font-black">{selectedCollection.name}</h2>
                <p class="text-xs text-base-content/65">{selectedCollection.sample_count.toLocaleString()} samples {selectedCollection.creator_username ? `· by ${selectedCollection.creator_username}` : ""}</p>
              </div>
            </div>
          </section>
        {:else if selectedPack}
          <section class="mb-2 overflow-hidden rounded-box border border-base-300 bg-base-200/40">
            <div class="pack-hero relative h-32 bg-base-300" style={selectedPack.cover_url ? `--pack-art: url('${selectedPack.banner_url || selectedPack.cover_url}')` : ""}>
              <div class="absolute inset-0 bg-gradient-to-r from-base-100 via-base-100/75 to-base-100/10"></div>
              <div class="absolute inset-0 flex items-end gap-3 p-3">
                {#if selectedPack.cover_url}
                  <img src={selectedPack.cover_url} alt="" class="h-18 w-18 rounded-box object-cover shadow" />
                {:else}
                  <div class="flex h-18 w-18 items-center justify-center rounded-box bg-neutral text-neutral-content shadow">
                    <Sparkles size={24} />
                  </div>
                {/if}
                <div class="min-w-0 pb-1">
                  <button class="btn btn-ghost btn-xs -ml-2 mb-1" onclick={() => { selectedPack = null; searchResult = null; }}>← Packs</button>
                  <h2 class="truncate text-lg font-black">{selectedPack.name}</h2>
                  <p class="text-xs text-base-content/65">{packSubtitle(selectedPack)}</p>
                </div>
              </div>
            </div>
          </section>
        {:else}
        <section class="mb-2 border-b border-base-300 bg-base-100 pb-2">
          <div>
            {#if activeView === "library"}
              <div class="tabs tabs-border tabs-sm mb-2">
                <button class="tab" class:tab-active={libraryTab === "samples"} onclick={() => { libraryTab = "samples"; switchView("library"); }}>Samples</button>
                <button class="tab" class:tab-active={libraryTab === "packs"} onclick={() => { libraryTab = "packs"; switchView("library"); }}>Packs</button>
              </div>
            {:else if activeView === "liked"}
              <div class="tabs tabs-border tabs-sm mb-2">
                <button class="tab" class:tab-active={likedTab === "samples"} onclick={() => { likedTab = "samples"; switchView("liked"); }}>Samples</button>
                <button class="tab" class:tab-active={likedTab === "packs"} onclick={() => { likedTab = "packs"; switchView("liked"); }}>Packs</button>
              </div>
            {/if}
            {#if (activeView !== "library" || libraryTab === "samples") && (activeView !== "liked" || likedTab === "samples")}
            <form
              class="grid grid-cols-1 items-center gap-2 md:grid-cols-[1fr_auto_auto]"
              onsubmit={(event) => {
                event.preventDefault();
                runSearch({ purchasedOnly: activeView === "library" ? true : onlyPurchased, filters: activeFilters(), force: true });
              }}
            >
              <label class="input input-sm input-bordered flex items-center gap-2">
                <span class="opacity-55">⌕</span>
                <input
                  class="grow"
                  type="text"
                  value={activeView === "library" ? libraryQuery : searchQuery}
                  oninput={(event) => {
                    if (activeView === "library") libraryQuery = event.currentTarget.value;
                    else searchQuery = event.currentTarget.value;
                  }}
                  placeholder={activeView === "library" || activeView === "liked" ? "Search your library…" : "Search sounds…"}
                />
              </label>

              <label class="label cursor-pointer justify-start gap-2 py-0 text-xs" class:hidden={activeView === "library" || activeView === "liked"}>
                <input type="checkbox" class="checkbox checkbox-primary checkbox-xs" bind:checked={onlyPurchased} />
                <span>Owned</span>
              </label>

              <button class="btn btn-primary btn-sm" disabled={searching || !connected}>
                {searching ? "Searching…" : "Search"}
              </button>
            </form>
            {/if}
          </div>
        </section>
        {/if}

        {#if ((activeView === "library" && libraryTab === "packs") || (activeView === "liked" && likedTab === "packs")) && !selectedPack}
          {@const visiblePacks = activeView === "liked" ? likedPacks : packs}
          {#if visiblePacks.length}
            <section class="grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-2">
              {#each visiblePacks as pack}
                <button class="pack-card card card-compact cursor-pointer overflow-hidden border border-base-300 bg-base-100 text-left shadow-sm transition hover:border-primary hover:bg-base-200" onclick={() => openPack(pack)}>
                  <figure class="aspect-square bg-base-200">
                    {#if pack.cover_url}<img src={pack.cover_url} alt="" class="h-full w-full object-cover" />{:else}<div class="flex h-full w-full items-center justify-center bg-neutral text-neutral-content"><Sparkles size={24} /></div>{/if}
                  </figure>
                  <div class="card-body gap-1 p-2"><strong class="line-clamp-2 text-xs leading-tight">{pack.name}</strong><span class="truncate text-[0.7rem] text-base-content/60">{packSubtitle(pack)}</span></div>
                </button>
              {/each}
            </section>
          {:else}
            <section class="hero min-h-80 rounded-box border border-base-300 bg-base-200/40"><div class="hero-content"><span class="loading loading-spinner loading-md text-primary"></span></div></section>
          {/if}
        {:else}

        <section class="filter-panel mb-2 rounded-box border border-base-300 bg-base-100/95 p-2 shadow-sm">
          <div class="flex flex-wrap items-center gap-1.5">
            <div class="mr-1 flex items-center gap-1.5 text-xs font-bold text-base-content/60">
              <SlidersHorizontal size={14} />
              Filters
            </div>
            <div class="dropdown dropdown-bottom relative z-20">
              <button class="filter-control btn btn-xs min-w-32 justify-between" class:filter-active={activeFilters().instruments.length > 0 || openFilterDropdown === "instruments"} onclick={(event) => toggleFilterDropdown("instruments", event)}>{multiLabel("Instrument", activeFilters().instruments)}</button>
              {#if openFilterDropdown === "instruments"}
                <div class="menu dropdown-content z-50 mt-1 w-44 rounded-box border border-base-300 bg-base-100 p-1 shadow-xl">
                  {#each instruments as item}<label class="label cursor-pointer justify-start gap-2 px-2 py-1 text-xs"><input type="checkbox" class="checkbox checkbox-xs checkbox-primary" checked={activeFilters().instruments.includes(item)} onchange={() => toggleFilterValue("instruments", item)} />{item}</label>{/each}
                </div>
              {/if}
            </div>
            <div class="dropdown dropdown-bottom relative z-20">
              <button class="filter-control btn btn-xs min-w-28 justify-between" class:filter-active={activeFilters().genres.length > 0 || openFilterDropdown === "genres"} onclick={(event) => toggleFilterDropdown("genres", event)}>{multiLabel("Genre", activeFilters().genres)}</button>
              {#if openFilterDropdown === "genres"}
                <div class="menu dropdown-content z-50 mt-1 w-44 rounded-box border border-base-300 bg-base-100 p-1 shadow-xl">
                  {#each genres as item}<label class="label cursor-pointer justify-start gap-2 px-2 py-1 text-xs"><input type="checkbox" class="checkbox checkbox-xs checkbox-primary" checked={activeFilters().genres.includes(item)} onchange={() => toggleFilterValue("genres", item)} />{item}</label>{/each}
                </div>
              {/if}
            </div>
            <div class="dropdown dropdown-bottom relative z-20">
              <button class="filter-control btn btn-xs min-w-24 justify-between" class:filter-active={activeFilters().keys.length > 0 || openFilterDropdown === "keys"} onclick={(event) => toggleFilterDropdown("keys", event)}>{multiLabel("Key", activeFilters().keys)}</button>
              {#if openFilterDropdown === "keys"}
                <div class="menu dropdown-content z-50 mt-1 w-36 rounded-box border border-base-300 bg-base-100 p-1 shadow-xl">
                  {#each keys as item}<label class="label cursor-pointer justify-start gap-2 px-2 py-1 text-xs"><input type="checkbox" class="checkbox checkbox-xs checkbox-primary" checked={activeFilters().keys.includes(item)} onchange={() => toggleFilterValue("keys", item)} />{item}</label>{/each}
                </div>
              {/if}
            </div>
            <div class="join">
              <label class="filter-input input join-item input-xs w-[4.75rem]"><input type="number" placeholder="Min BPM" value={activeFilters().bpmMin} oninput={(event) => updateFilter("bpmMin", event.currentTarget.value)} /></label>
              <label class="filter-input input join-item input-xs w-[4.75rem]"><input type="number" placeholder="Max BPM" value={activeFilters().bpmMax} oninput={(event) => updateFilter("bpmMax", event.currentTarget.value)} /></label>
            </div>
            <div class="join ml-1">
              <button class="filter-control btn join-item btn-xs" class:filter-active={activeFilters().sampleType === ""} onclick={() => setSampleType("")}>All</button>
              <button class="filter-control btn join-item btn-xs" class:filter-active={activeFilters().sampleType === "one_shot"} onclick={() => setSampleType("one_shot")}>One-shots</button>
              <button class="filter-control btn join-item btn-xs" class:filter-active={activeFilters().sampleType === "loop"} onclick={() => setSampleType("loop")}>Loops</button>
            </div>
            <div class="ml-auto flex items-center gap-1.5">
              <span class="text-[0.65rem] font-bold uppercase tracking-wide text-base-content/45">Sort</span>
              <select class="filter-select select select-xs w-32" value={activeFilters().sort} onchange={(event) => updateFilter("sort", event.currentTarget.value)}>
                {#each sorts as item}<option value={item}>{sortLabel(item)}</option>{/each}
              </select>
            </div>
          </div>
          <div class="mt-2 flex min-w-0 flex-wrap gap-1 overflow-hidden border-t border-base-300/70 pt-2">
            {#each activeFilters().tags as tag}
              <button class="btn btn-primary btn-xs h-6 min-h-6 gap-1 rounded-full px-2" onclick={() => removeTag(tag)}>{tag}<X size={12} /></button>
            {/each}
            {#each Object.entries(searchResult?.matching_tags ?? {}).slice(0, 12) as [tag, count]}
              {#if !activeFilters().tags.includes(tag)}
                <button class="btn btn-outline btn-xs h-6 min-h-6 rounded-full px-2 font-medium" onclick={() => addTag(tag)}>{tag}<span class="ml-1 opacity-50">{count}</span></button>
              {/if}
            {/each}
          </div>
        </section>

        {#if searching && !searchResult}
          <section class="hero min-h-80 rounded-box border border-base-300 bg-base-200/40">
            <div class="hero-content text-center">
              <div>
                <span class="loading loading-spinner loading-md text-primary"></span>
                <h2 class="mt-3 text-2xl font-black">Loading sounds…</h2>
              </div>
            </div>
          </section>
        {:else if searchResult}
          <section class="overflow-hidden rounded-box border border-base-300 bg-base-200/25">
            <div class="sample-table">
              <div class="table-row table-head">
                <div>Pack</div>
                <div></div>
                <button class="table-sort" onclick={() => sortBy("filename")}>Filename ↕</button>
                <div>Waveform</div>
                <div>Time</div>
                <button class="table-sort" onclick={() => sortBy("key")}>Key ↕</button>
                <button class="table-sort" onclick={() => sortBy("bpm")}>BPM ↕</button>
                <div class="sticky-head">Actions</div>
              </div>

              {#each searchResult.samples as sample}
                <div class="table-row sample-row" class:active={playingHash === sample.file_hash} class:local={sample.local_path} onclick={() => play(sample)} role="button" tabindex="0" onkeydown={(event) => { if (event.key === "Enter" || event.key === " ") play(sample); }}>
                  <button class="avatar placeholder pack-art-button" onclick={(event) => openSamplePack(sample, event)} disabled={!sample.pack_uuid} aria-label={sample.pack_name ? `Open ${sample.pack_name}` : "Open pack"}>
                    {#if sampleCover(sample)}
                      <span class="h-7 w-7 overflow-hidden rounded-btn">
                        <img src={sampleCover(sample)} alt="" class="h-full w-full object-cover" />
                      </span>
                    {:else}
                      <span class="h-7 w-7 rounded-btn bg-neutral text-neutral-content">
                        <span class="text-[0.65rem] font-black">{initials(sample)}</span>
                      </span>
                    {/if}
                  </button>
                  <div class="row-play-indicator">
                    <span class="idle-icon">
                      {#if playedHashes.includes(sample.file_hash)}<RotateCcw size={17} />{:else}<AudioWaveform size={17} />{/if}
                    </span>
                    <span class="hover-play">
                      {#if playingHash === sample.file_hash}<Square size={15} fill="currentColor" />{:else}<Play size={18} fill="currentColor" />{/if}
                    </span>
                  </div>
                  <div class="filename-cell">
                    <strong>{sample.filename}</strong>
                    <small>
                      {sampleTagLabel(sample)}
                      {#if previewFailures.includes(sample.file_hash) && !sample.local_path}
                        · remote preview unsupported
                      {/if}
                    </small>
                  </div>
                  <div class="waveform" class:real-waveform={!!waveformData[sample.file_hash]?.length} aria-label="Preview waveform">
                    {#each waveformBars(sample) as height, index}
                      <span class:played={index / 44 <= progressFor(sample)} style={`height: ${height}%`}></span>
                    {/each}
                  </div>
                  <div class="time">{formatTime(sample.duration || duration)}</div>
                  <div class="key">{sample.key || "--"}</div>
                  <div class="bpm">{sample.bpm || "--"}</div>
                  <div class="row-actions sticky-actions">
                    {#if sample.local_path}
                      <div class="tooltip tooltip-left" data-tip="Drag to DAW">
                        <button class="btn btn-info btn-square btn-xs" onpointerdown={(event) => startFileDrag(event, sample)} onclick={(event) => event.stopPropagation()} aria-label="Drag to DAW">
                          {#if draggingHash === sample.file_hash}
                            <span class="loading loading-spinner loading-xs"></span>
                          {:else}
                            <Grip size={14} />
                          {/if}
                        </button>
                      </div>
                      <div class="tooltip tooltip-left" data-tip="Reveal in Finder">
                        <button class="btn btn-ghost btn-square btn-xs" onclick={(event) => { event.stopPropagation(); reveal(sample); }} aria-label="Reveal in Finder"><FolderOpen size={14} /></button>
                      </div>
                          {:else}
                      <div class="tooltip tooltip-left" data-tip={sample.purchased ? "Download" : `Buy for ${sample.price} credit${sample.price === 1 ? "" : "s"}`}>
                        <button class="btn btn-primary btn-square btn-xs" onclick={(event) => { event.stopPropagation(); downloadSample(sample); }} disabled={downloadingHash === sample.file_hash} aria-label={sample.purchased ? "Download" : "Buy"}>
                          {#if downloadingHash === sample.file_hash}
                            <span class="loading loading-spinner loading-xs"></span>
                          {:else if sample.purchased}
                            <Download size={14} />
                          {:else}
                            <ShoppingCart size={14} />
                          {/if}
                        </button>
                      </div>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
          </section>
          <div class="flex items-center justify-center py-3">
            {#if hasMoreSamples}
              <button class="btn btn-ghost btn-sm" onclick={loadMoreSamples} disabled={loadingMore}>
                {loadingMore ? "Loading…" : `Load more (${searchResult.samples.length.toLocaleString()} / ${searchResult.total_hits.toLocaleString()})`}
              </button>
            {:else if searchResult.samples.length > 0}
              <span class="text-xs text-base-content/50">End of results</span>
            {/if}
          </div>
        {:else}
          <section class="hero min-h-80 rounded-box border border-base-300 bg-base-200/40">
            <div class="hero-content text-center">
              <div>
                <h2 class="text-2xl font-black">{activeView === "library" ? "Search your purchased sounds." : "Search Splice."}</h2>
                <p class="mt-2 text-base-content/60">Results appear as a compact table. Downloaded rows can be dragged straight into your DAW.</p>
              </div>
            </div>
          </section>
        {/if}
        {/if}
      {:else if activeView === "collections"}
        <section class="mb-2 flex items-center justify-between border-b border-base-300 pb-2">
          <p class="text-xs text-base-content/60">Your saved collections.</p>
          <button class="btn btn-ghost btn-xs" onclick={() => loadCollections({ force: true })} disabled={loadingCollections}>{loadingCollections ? "Loading…" : "Refresh"}</button>
        </section>
        {#if collections.length}
          <section class="grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-2">
            {#each collections as collection}
              <button class="pack-card card card-compact cursor-pointer overflow-hidden border border-base-300 bg-base-100 text-left shadow-sm transition hover:border-primary hover:bg-base-200" onclick={() => openCollection(collection)}>
                <figure class="aspect-square bg-base-200">
                  {#if collection.cover_url}<img src={collection.cover_url} alt="" class="h-full w-full object-cover" />{:else}<div class="flex h-full w-full items-center justify-center bg-neutral text-neutral-content"><Archive size={24} /></div>{/if}
                </figure>
                <div class="card-body gap-1 p-2">
                  <strong class="line-clamp-2 text-xs leading-tight">{collection.name}</strong>
                  <span class="truncate text-[0.7rem] text-base-content/60">{collection.sample_count.toLocaleString()} samples</span>
                </div>
              </button>
            {/each}
          </section>
        {:else}
          <section class="hero min-h-80 rounded-box border border-base-300 bg-base-200/40"><div class="hero-content text-center"><div><h2 class="text-2xl font-black">{loadingCollections ? "Loading collections…" : "No collections yet."}</h2></div></div></section>
        {/if}
      {:else if activeView === "packs"}
        <section class="mb-2 flex items-center justify-between border-b border-base-300 pb-2">
          <p class="text-xs text-base-content/60">Curated by Splice and updated daily.</p>
          <button class="btn btn-ghost btn-xs" onclick={() => loadPacks({ force: true })} disabled={loadingPacks}>{loadingPacks ? "Loading…" : "Refresh"}</button>
        </section>

        {#if packs.length}
          <section class="grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-2">
            {#each packs as pack}
              <button class="pack-card card card-compact cursor-pointer overflow-hidden border border-base-300 bg-base-100 text-left shadow-sm transition hover:border-primary hover:bg-base-200" onclick={() => openPack(pack)}>
                <figure class="aspect-square bg-base-200">
                  {#if pack.cover_url}
                    <img src={pack.cover_url} alt="" class="h-full w-full object-cover" />
                  {:else}
                    <div class="flex h-full w-full items-center justify-center bg-neutral text-neutral-content">
                      <span class="text-lg font-black">{pack.name.slice(0, 2).toUpperCase()}</span>
                    </div>
                  {/if}
                </figure>
                <div class="card-body gap-1 p-2">
                  <strong class="line-clamp-2 text-xs leading-tight">{pack.name}</strong>
                  <span class="truncate text-[0.7rem] text-base-content/60">{packSubtitle(pack)}</span>
                </div>
              </button>
            {/each}
          </section>
        {:else}
          <section class="hero min-h-80 rounded-box border border-base-300 bg-base-200/40">
            <div class="hero-content text-center">
              <div>
                <h2 class="text-2xl font-black">{loadingPacks ? "Loading packs…" : "No packs loaded."}</h2>
                <p class="mt-2 text-base-content/60">Use Refresh to try again.</p>
              </div>
            </div>
          </section>
        {/if}
      {:else}
        <section class="hero min-h-96 rounded-box border border-base-300 bg-base-200/40">
          <div class="hero-content max-w-xl text-center">
            <div>
              <h2 class="text-3xl font-black">{viewTitle} is next.</h2>
              <p class="mt-3 text-base-content/60">Collections and liked sounds are next.</p>
              <button class="btn btn-ghost btn-sm mt-4" onclick={() => switchView("search")}>Back to search</button>
            </div>
          </div>
        </section>
      {/if}
    </main>

    <footer class="fixed bottom-0 right-0 left-44 z-30 grid h-12 grid-cols-[minmax(220px,1fr)_auto_auto] items-center gap-3 border-t border-base-300 bg-base-100/95 px-3 backdrop-blur">
      <div class="flex min-w-0 items-center gap-3">
        <button class="btn btn-circle btn-primary btn-xs" onclick={playingHash ? () => stopPlayback() : resume} disabled={!currentSample}>
          {playingHash ? "■" : "▶"}
        </button>
        <div class="min-w-0">
          <strong class="block truncate text-sm">{currentSample?.filename ?? "No preview selected"}</strong>
          <small class="block truncate text-xs text-base-content/60">{currentSample?.provider_name ?? "Preview a sound to start playback"}</small>
        </div>
      </div>

      <label class="hidden items-center gap-2 text-xs text-base-content/70 md:flex">
        Volume
        <input
          class="range range-primary range-xs w-32"
          type="range"
          min="0"
          max="1"
          step="0.01"
          value={volume}
          oninput={(event) => setVolume(Number(event.currentTarget.value))}
        />
      </label>

      <div>
        {#if currentSample?.local_path}
          <button class="btn btn-info btn-xs" onpointerdown={(event) => startFileDrag(event, currentSample!)}>Drag to DAW</button>
        {/if}
      </div>
    </footer>
  </div>

  <aside class="fixed left-0 top-0 z-40 flex h-screen w-44 flex-col border-r border-base-300 bg-base-200 p-2">
      <div class="mb-1 flex items-center gap-2 px-2 py-1">
        <div class="flex h-7 w-7 items-center justify-center rounded-full bg-primary/15 text-base leading-none">🧁</div>
        <div class="leading-none">
          <strong class="block text-sm leading-tight">Supuraisu</strong>
          <small class="block text-[0.65rem] leading-none text-base-content/55">Client for Splice</small>
        </div>
      </div>

      <ul class="menu menu-xs mt-2 w-full gap-0 p-0">
        {#each navItems as item}
          {@const Icon = item.icon}
          {@render sidebarItem(item.label, Icon, activeView === item.id, () => switchView(item.id))}
        {/each}
      </ul>

      <section class="sidebar-subnav mt-3 min-h-0 overflow-hidden border-t border-base-300 pt-2">
        <div class="mb-1 flex items-center justify-between px-2 text-[0.65rem] font-bold uppercase tracking-wide text-base-content/45">
          <span>Collections</span>
          <button class="btn btn-ghost btn-square btn-xs" onclick={() => loadCollections({ force: true })} aria-label="Refresh collections">
            <RefreshCw size={12} class={loadingCollections ? "animate-spin" : ""} />
          </button>
        </div>
        <ul class="menu menu-xs w-full gap-0 p-0">
          {@render sidebarItem("All Collections", Archive, activeView === "collections" && !selectedCollection, () => { activeView = "collections"; selectedCollection = null; searchResult = null; loadCollections(); })}
          {@render sidebarItem("Likes", Heart, activeView === "liked", () => switchView("liked"))}
          <div class="mt-1 max-h-64 overflow-y-auto pr-1">
            {#each collections as collection}
              {@render sidebarItem(collection.name, Archive, selectedCollection?.uuid === collection.uuid, () => { activeView = "collections"; openCollection(collection); })}
            {/each}
          </div>
        </ul>
      </section>

      <div class="mt-auto grid gap-1">
        <div class="account-card group min-w-0 overflow-hidden rounded-box border border-base-300 bg-base-100 p-2">
          <div class="grid gap-0.5">
            <div class="flex items-center gap-2">
              <span class="badge badge-xs {connected ? 'badge-success' : 'badge-error'}"></span>
              <strong class="min-w-0 flex-1 truncate text-sm">{ownedAuth?.signed_in ? ownedAuth.username : account?.user?.username ?? (loadingAccount || authLoading ? "Connecting…" : "Offline")}</strong>
            </div>
            <div class="flex items-center justify-between gap-2">
              <span class="min-w-0 truncate text-xs text-base-content/60">{credits === null ? "Credits unavailable" : `${credits.toLocaleString()} credits`}</span>
              <button class="btn btn-ghost btn-square btn-xs shrink-0" onclick={loadAccount} disabled={loadingAccount || authLoading} aria-label="Refresh account">
                <RefreshCw size={13} class={loadingAccount || authLoading ? "animate-spin" : ""} />
              </button>
            </div>
            <div class="mt-1 grid grid-cols-2 gap-1">
              <button class="btn btn-ghost btn-xs" onclick={openDiagnostics}>About</button>
              <button class="btn btn-outline btn-xs" onclick={signOut} disabled={authLoading}>Log out</button>
            </div>
          </div>
        </div>
      </div>
  </aside>
  {/if}

  {#if diagnosticsOpen}
    <div class="modal modal-open">
      <div class="modal-box max-w-2xl border border-base-300 bg-base-100">
        <form method="dialog"><button class="btn btn-ghost btn-sm btn-circle absolute right-2 top-2" onclick={() => diagnosticsOpen = false}>✕</button></form>
        <div class="mb-4 flex items-center gap-3">
          <div class="flex h-12 w-12 items-center justify-center rounded-2xl bg-primary/15 text-3xl">🧁</div>
          <div>
            <h2 class="text-xl font-black">Supuraisu</h2>
            <p class="text-xs text-base-content/60">Version {diagnostics?.app_version ?? "—"}</p>
          </div>
        </div>
        {#if loadingDiagnostics}
          <div class="flex items-center gap-2 text-sm"><span class="loading loading-spinner loading-sm"></span> Loading diagnostics…</div>
        {:else if diagnostics}
          <div class="grid gap-3 text-xs">
            <section class="rounded-box border border-base-300 bg-base-200/40 p-3">
              <h3 class="mb-2 font-black uppercase tracking-wide text-base-content/60">Session</h3>
              <dl class="grid grid-cols-[9rem_1fr] gap-x-3 gap-y-1">
                <dt>Keychain consent</dt><dd>{diagnostics.keychain_consent ? "Yes" : "No"}</dd>
                <dt>Supuraisu auth</dt><dd>{diagnostics.owned_signed_in ? `Signed in as ${diagnostics.owned_username || diagnostics.owned_email}` : "Signed out"}</dd>
                <dt>Helper</dt><dd>{diagnostics.helper_connected ? diagnostics.helper_has_token ? "Connected with token" : "Connected" : "Not connected"}</dd>
                <dt>Helper user</dt><dd>{diagnostics.helper_username || diagnostics.helper_email || "—"}</dd>
                <dt>Helper synced</dt><dd>{diagnostics.helper_synced ? "Yes" : "No"}</dd>
              </dl>
            </section>
            <section class="rounded-box border border-base-300 bg-base-200/40 p-3">
              <h3 class="mb-2 font-black uppercase tracking-wide text-base-content/60">Environment</h3>
              <dl class="grid grid-cols-[9rem_1fr] gap-x-3 gap-y-1 break-all">
                <dt>Splice.app</dt><dd>{diagnostics.environment.app_installed ? "Installed" : "Missing"} · {diagnostics.environment.app_path}</dd>
                <dt>Helper</dt><dd>{diagnostics.environment.helper_installed ? "Installed" : "Missing"} · {diagnostics.environment.helper_path}</dd>
                <dt>User data</dt><dd>{diagnostics.environment.user_data_path ?? "—"}</dd>
                <dt>Cert</dt><dd>{diagnostics.environment.cert_exists ? "OK" : "Missing"} · {diagnostics.environment.cert_path ?? "—"}</dd>
                <dt>Key</dt><dd>{diagnostics.environment.key_exists ? "OK" : "Missing"} · {diagnostics.environment.key_path ?? "—"}</dd>
                <dt>gRPC scan</dt><dd>{diagnostics.environment.grpc_port_start}–{diagnostics.environment.grpc_port_end}</dd>
              </dl>
            </section>
            <section class="rounded-box border border-base-300 bg-base-200/40 p-3">
              <h3 class="mb-2 font-black uppercase tracking-wide text-base-content/60">Scrambled previews</h3>
              <dl class="grid grid-cols-[9rem_1fr] gap-x-3 gap-y-1 break-all">
                <dt>Decoder</dt>
                <dd>
                  {#if decoderDiagnostics?.status === "ready"}
                    Ready · using installed Splice WASM
                  {:else if decoderDiagnostics?.status === "loading"}
                    Checking…
                  {:else if decoderDiagnostics?.status === "missing"}
                    Missing
                  {:else if decoderDiagnostics?.status === "incompatible"}
                    Incompatible
                  {:else}
                    Error
                  {/if}
                </dd>
                <dt>Candidates</dt><dd>{decoderDiagnostics?.candidate_count ?? 0}</dd>
                <dt>Active WASM</dt><dd>{decoderDiagnostics?.compatible_path || "—"}</dd>
                <dt>Size</dt><dd>{decoderDiagnostics?.compatible_size ? `${(decoderDiagnostics.compatible_size / 1024 / 1024).toFixed(2)} MB` : "—"}</dd>
                {#if decoderDiagnostics?.candidate_paths.length}
                  <dt>Found</dt><dd>{decoderDiagnostics.candidate_paths.slice(0, 4).join(", ")}</dd>
                {/if}
                {#if decoderDiagnostics?.error}
                  <dt>Error</dt><dd>{decoderDiagnostics.error}</dd>
                {/if}
              </dl>
            </section>
            {#if diagnostics.errors.length}
              <section class="rounded-box border border-error/30 bg-error/10 p-3 text-error">
                <h3 class="mb-2 font-black uppercase tracking-wide">Diagnostics</h3>
                <ul class="list-disc pl-4">{#each diagnostics.errors.slice(0, 6) as item}<li>{item}</li>{/each}</ul>
              </section>
            {/if}
          </div>
        {/if}
      </div>
      <button class="modal-backdrop" onclick={() => diagnosticsOpen = false}>close</button>
    </div>
  {/if}
</div>

<style>
  :global(aside .menu),
  :global(aside .menu li),
  :global(aside .menu li > button) {
    width: 100%;
    max-width: none;
  }

  :global(aside .menu li > button) {
    display: flex;
    justify-content: flex-start;
  }

  :global(aside .sidebar-nav-item) {
    border: 1px solid transparent;
    color: color-mix(in oklab, var(--color-base-content) 72%, transparent);
  }

  :global(aside .sidebar-nav-item:hover) {
    border-color: var(--color-base-300);
    background: color-mix(in oklab, var(--color-base-100) 72%, transparent);
    color: var(--color-base-content);
  }

  :global(aside .sidebar-nav-item.sidebar-active) {
    border-color: color-mix(in oklab, var(--color-neutral) 45%, var(--color-base-300));
    background: color-mix(in oklab, var(--color-neutral) 14%, var(--color-base-100));
    color: var(--color-base-content);
    font-weight: 900;
  }

  :global(aside .sidebar-nav-item.sidebar-active svg) {
    stroke-width: 2.7;
  }

  .pack-card {
    cursor: pointer;
  }

  .filter-control,
  .filter-input,
  .filter-select {
    border-color: var(--color-base-300);
    background: var(--color-base-100);
    color: var(--color-base-content);
    font-weight: 700;
  }

  .filter-control {
    box-shadow: none;
  }

  .filter-control:hover,
  .filter-control.filter-active,
  .filter-select:focus,
  .filter-input:focus-within {
    border-color: var(--color-primary);
    background: color-mix(in oklab, var(--color-primary) 8%, var(--color-base-100));
    color: var(--color-base-content);
  }

  .filter-control.filter-active {
    color: var(--color-primary);
  }

  .pack-hero {
    background-image: var(--pack-art);
    background-position: center;
    background-size: cover;
  }

  .sample-table {
    width: 100%;
  }

  .table-row {
    display: grid;
    grid-template-columns: 42px 42px minmax(170px, 1.35fr) minmax(120px, 0.9fr) 48px 44px 44px 76px;
    align-items: center;
    min-width: 0;
    border: 0;
    border-bottom: 1px solid color-mix(in oklab, currentColor 12%, transparent);
    background: transparent;
    color: inherit;
    text-align: left;
  }

  .table-row > * {
    min-width: 0;
    padding: 0.24rem 0.45rem;
  }

  .table-sort {
    appearance: none;
    border: 0;
    background: transparent;
    color: inherit;
    cursor: pointer;
    font: inherit;
    letter-spacing: inherit;
    text-align: left;
    text-transform: inherit;
  }

  .table-head {
    position: sticky;
    top: 0;
    z-index: 3;
    background: var(--color-base-200);
    color: color-mix(in oklab, var(--color-base-content) 62%, transparent);
    font-size: 0.62rem;
    font-weight: 900;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .sample-row {
    width: 100%;
    cursor: pointer;
  }

  .pack-art-button {
    border: 0;
    background: transparent;
    padding: 0;
    cursor: pointer;
    transition: transform 120ms ease, filter 120ms ease;
  }

  .pack-art-button:hover:not(:disabled) {
    transform: scale(1.06);
    filter: brightness(1.05) saturate(1.08);
  }

  .pack-art-button:disabled {
    cursor: default;
  }

  .table-row:not(.table-head):hover,
  .table-row.active {
    background: color-mix(in oklab, var(--color-primary) 12%, transparent);
  }

  .row-play-indicator {
    display: grid;
    place-items: center;
    color: color-mix(in oklab, var(--color-base-content) 48%, transparent);
  }

  .row-play-indicator .hover-play {
    display: none;
    place-items: center;
    width: 2rem;
    height: 2rem;
    border-radius: 0.7rem;
    border: 2px solid color-mix(in oklab, var(--color-primary) 65%, var(--color-base-100));
    background: color-mix(in oklab, var(--color-primary) 10%, var(--color-base-100));
    color: var(--color-primary);
  }

  .sample-row:hover .row-play-indicator .idle-icon,
  .sample-row.active .row-play-indicator .idle-icon {
    display: none;
  }

  .sample-row:hover .row-play-indicator .hover-play,
  .sample-row.active .row-play-indicator .hover-play {
    display: grid;
  }

  .table-row.local {
    background-image: linear-gradient(90deg, color-mix(in oklab, var(--color-success) 10%, transparent), transparent);
  }

  .filename-cell {
    position: relative;
    overflow: hidden;
  }

  .filename-cell::after {
    content: "";
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    width: 2.5rem;
    background: linear-gradient(90deg, transparent, var(--color-base-100) 80%);
    pointer-events: none;
  }

  .table-row:hover .filename-cell::after,
  .table-row.active .filename-cell::after {
    background: linear-gradient(90deg, transparent, color-mix(in oklab, var(--color-primary) 12%, var(--color-base-100)) 80%);
  }

  .filename-cell strong,
  .filename-cell small {
    display: block;
    overflow: hidden;
    white-space: nowrap;
  }

  .filename-cell strong {
    font-size: 0.8rem;
  }

  .filename-cell small {
    color: color-mix(in oklab, var(--color-base-content) 58%, transparent);
  }

  .waveform {
    display: flex;
    align-items: center;
    gap: 2px;
    width: 100%;
    max-width: 100%;
    height: 1.45rem;
    padding: 0;
    overflow: hidden;
    border-radius: 0;
    background: transparent;
    contain: paint;
  }

  .waveform span {
    flex: 1 1 0;
    min-width: 1px;
    max-width: 3px;
    border-radius: 999px;
    background: color-mix(in oklab, var(--color-base-content) 28%, transparent);
  }

  .waveform span.played,
  .table-row.active .waveform span.played {
    background: var(--color-primary);
  }

  .waveform.real-waveform span {
    background: color-mix(in oklab, var(--color-base-content) 38%, transparent);
  }

  .time,
  .key,
  .bpm {
    color: color-mix(in oklab, var(--color-base-content) 75%, transparent);
    font-size: 0.78rem;
  }

  .row-actions {
    display: grid;
    grid-template-columns: repeat(2, 28px);
    gap: 0.25rem;
    align-items: center;
    justify-content: end;
  }

  .row-actions .btn {
    min-height: 1.75rem;
    height: 1.75rem;
    white-space: nowrap;
  }

  .row-actions .btn-square {
    width: 1.75rem;
    min-width: 1.75rem;
    padding: 0;
  }

  .sticky-actions,
  .sticky-head {
    position: sticky;
    right: 0;
    z-index: 2;
    background: linear-gradient(90deg, transparent, var(--color-base-100) 18px, var(--color-base-100));
    box-shadow: -18px 0 22px var(--color-base-100);
  }

  .table-row:hover .sticky-actions,
  .table-row.active .sticky-actions {
    background: linear-gradient(90deg, transparent, color-mix(in oklab, var(--color-primary) 12%, var(--color-base-100)) 18px, color-mix(in oklab, var(--color-primary) 12%, var(--color-base-100)));
    box-shadow: -18px 0 22px color-mix(in oklab, var(--color-primary) 12%, var(--color-base-100));
  }

  @media (max-width: 1024px) {
    .table-row {
      grid-template-columns: 40px 40px minmax(150px, 1fr) minmax(100px, 0.7fr) 48px 44px 44px 76px;
    }
  }
</style>
