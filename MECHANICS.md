# Mechanics read from the original

Source: FreeLens 2.0.0 (MIT licence), installed as Flatpak `app.freelens.Freelens`.
The desktop shortcut only launches `Freelens/freelens`, which is the stock Electron/Chromium
runtime (stripped ELF, evidence `ev_a3b1522e…`). FreeLens's own logic is JavaScript inside
`resources/app.asar` (extracted, evidence `ev_b8e1ec4a…`, finding `fnd_1`), so the "addresses"
below are **file:line in the renderer bundle** `static/build/assets/index-qzQrg_7k.js` plus the
original source module named in the bundle's `#region` comments. Native pseudocode of the
Chromium binary does not contain any of these rules, so it was not used.

CSS values come from `static/build/assets/index-Mav7N3RQ.css`.

## Node usage in the Nodes list — `packages/core/src/renderer/components/nodes/route.tsx`

| What | Rule in the original | Where | In this app |
|---|---|---|---|
| Refresh | `interval(30, …)`: every **30 s** reload node metrics (`nodeStore.loadKubeMetrics()` → `GET /apis/metrics.k8s.io/v1beta1/nodes`); starts immediately on open | 517688, `interval` 6591 (`setInterval(tick, 1e3 * timeSec)`), metrics API base 281941, store 433354 | `METRICS_REFRESH = 30 s` (`app.rs`), `kubectl get --raw /apis/metrics.k8s.io/v1beta1/nodes` |
| Usage per node | metrics `usage.cpu` (cores) / `usage.memory` (bytes); node without metrics → `NaN` | `getNodeKubeMetrics` 433371 | `node_usage` map; missing → `NaN` |
| Pods counted | pods with `spec.nodeName` and phase not `Succeeded`/`Failed` | `podsByNode` 517708 | `model::node_requests` |
| Requests | sum of `resources.requests.cpu/memory` over those pods' containers | `getNodeResourceRequests` 517729 | `model::node_requests` |
| Bar maximum | `capacity = promCapacity \|\| allocatable` → without Prometheus: **`status.allocatable`** | `renderCpuUsage` 517785, `renderMemoryUsage` 517809 | allocatable |
| Bar layers | LineProgress: `value` = usage, `secondaryValue` = requests drawn under it at **opacity 0.5**; width % = `min(100, value / (max − min) × 100)` | `renderUsage` 517754, `valuePercent` 422678, CSS `.LineProgress .line.secondary { opacity: 0.5 }` | `model::meter`, `views::paint_meter` |
| When a bar is shown | `hasBar = capacity && (usage finite \|\| requests > 0)`; no bar and no text → `"N/A"` | 517754 | same |
| CPU text | pods loaded: `formatCores(usage) + " / " + formatCores(requests)`, else `formatCores(usage)`; `formatCores`: NaN → `"N/A"`, `< 10` → 2 decimals, else 1 decimal | 517785, `formatCores` 517677 | `model::format_cores` |
| Memory text | `bytesToUnitsAligned(usage) + " / " + bytesToUnitsAligned(requests)`; `< 1024 B` → `(b/1024).toFixed(1)+"Ki"`, else `bytesToUnits(b, precision 1)` without trailing `B` (`5.9Gi`) | 517809, 517648; `bytesToUnits` 6026 (powers of 1024, `B KiB MiB GiB TiB PiB EiB`, ≤ 0 or NaN → `"N/A"`) | `model::bytes_to_units[_aligned]` |
| CPU tooltip | `CPU: {usage×100/capacity:.2}%, cores: {cap}` / `Usage: {x} cores` / `Requests: {r} cores ({r×100/alloc:.0}% of allocatable {alloc})` | 517785 | `node_cpu_meter` |
| Memory tooltip | `Memory: {pct:.2}%, {usage, 3 decimals}` / `Usage: …` / `Requests: … (N% of allocatable …)` | 517809 | `node_memory_meter` |
| Disk | Prometheus only (`fsUsage`, `fsSize`) → `N/A` without Prometheus | `renderDiskUsage` 517865 | `"N/A"` |
| Columns | Name, CPU, Memory, Disk, (Pods, Instance Type, Node Group, Capacity hidden by default), Roles, Taints, Version, Internal IP, Age, Schedulable, Conditions | `renderTableHeader` / `defaultHiddenTableColumns` in route.tsx | same visible set |
| Column weights | CSS flex: cpu/memory/disk/pods/taints/schedulable **0.6**, roles **1.5**, internalIp **1.3**, age **0.4**, conditions **1.7** | `.Nodes .TableCell.*` | `Kind::Node` columns; Name and Version **1.0 (estimated, no rule found)** |
| Bar colours | cpu `--blue` `#00a7a0`, memory `--magenta` `#c93dce`, disk `--golden` `#ffc63d`, pods `--primary`; bar 5 px high; text `--font-size-small` (12 px), right-aligned, 4 px gap | `.Nodes .TableCell.cpu .LineProgress` etc. | `theme::bar`, `paint_meter` |
| Condition colours | Ready `--colorSuccess` `#43a047`, MemoryPressure `#901995`, DiskPressure `#cc8d00`, PidPressure `#068dbf`, NetworkUnavailable `#812727`, OutOfDisk `#ce3933`, SchedulingDisabled `#ff9800` | `.Nodes .TableCell.conditions .condition.*` | `model::node_conditions`, `theme::tone` |

**Differences kept on purpose**
- Sorting CPU/Memory: the original sorts by Prometheus values, which do nothing without Prometheus; here they sort by metrics-server usage.
- Columns have a minimum width (header text, 150 px for Name, 120 px for usage bars) so the text isn't cut off in small windows. The original lets cells clip.
- Bar track colour `lineProgressBackground #414448` comes from the theme key name. The `.LineProgress` base rule that would use it wasn't printed (estimated).

## Theme — `packages/core/src/renderer/themes/lens-dark.injectable.ts` (line 526585)

primary / blue `#00a7a0`, magenta `#c93dce`, golden `#ffc63d`, textColorPrimary `#8e9297`,
textColorSecondary `#a0a0a0`, textColorTertiary `#909ba6`, textColorAccent `#ffffff`,
borderColor `#4c5053`, borderFaintColor `#373a3e`, mainBackground `#1e2124`,
contentColor `#262b2f`, layoutBackground `#2e3136` (top bar), sidebarBackground `#36393e`,
sidebarItemHoverBackground `#3a3e44`, active sidebar item `--primary` + `#ffffff`,
clusterMenuBackground `#252729` (hotbar), clusterMenuCellBackground `#2e3136`,
tableBgcStripe `#2a2d33` (odd rows), tableBgcSelected `#383c42` (hover + selected),
tableHeaderBackground `#262b2f`, tableHeaderColor `#ffffff`, colorSuccess `#43a047`,
colorOk `#4caf50`, colorError `#ce3933`, colorSoftError `#e85555`, colorWarning `#ff9800`,
colorTerminated `#4c5053`, dockHeadBackground `#2e3136`, dockTabActiveBackground `#3a3e45`,
logsBackground `#000000`, logsForeground `#ffffff`, dialogBackground `#262b2f`,
drawer title background `--blue`, drawerTitleText `#ffffff`, drawerSubtitleBackground `#373a3e`,
drawerItemNameColor `#87909c`, drawerItemValueColor `#a0a0a0`, inputControlBackground `#1e2125`,
inputControlBorder `#414448`, inputControlHoverBorder `#474a4f`, buttonDefaultBackground `#414448`,
status bar background `--primary`, text white. → `theme.rs`

## Sizes

| Value | Original | Where | In this app |
|---|---|---|---|
| Hotbar width | 75 px | CSS `--hotbar-width` | `HOTBAR_WIDTH` |
| Hotbar cell | 40 × 40, margin 12, radius 6 | `.HotbarMenu .HotbarItems .HotbarCell` | `HOTBAR_CELL_*` |
| Top bar height | 40 px | `--main-layout-header` | `TOPBAR_HEIGHT` |
| Status bar height | 21 px, font-size-small | `--status-bar-height` | `STATUSBAR_HEIGHT` |
| Sidebar width | 200 px default | sidebar storage, line 422143 | `SIDEBAR_WIDTH` |
| Details drawer width | 725 px default, max 95 % of the view | drawer storage line 303118; `.Drawer { max-width: 95vw }` | `DRAWER_WIDTH` |
| Drawer title | min-height 54 px, padding 8 × 20, bold | `.Drawer .drawer-title` | `DRAWER_TITLE_HEIGHT` |
| Fonts | 14 px body, 12 px small (unit 8 px × 1.75 / 1.5), Roboto | `--font-size`, `--font-size-small`, `--font-main` | 14 / 12 px; egui's built-in font (Roboto not bundled) |
| Border radius | 3 px | `--border-radius` | widget corner radius 3 |

## Labels in the details drawer — `drawer/drawer-item-labels.tsx` (line 303308), `badge/badge.tsx` (line 287020)

| What | Original | In this app |
|---|---|---|
| Label text | `KubeObject.stringifyLabels` → `key=value`, one Badge per label, `title` = the label | same (`details::chip_flow`) |
| Badge look | background `--colorVague` `#36393e`, radius 3 px, padding `0.2rem 0.4rem` (3.2 × 6.4 px), `.DrawerItem.labelsOnly > .value > div { float: left; margin: 4px }` → 8 px between badges, text in `drawerItemValueColor` `#a0a0a0` | same |
| Overflow | single line, `white-space: nowrap; text-overflow: ellipsis`; when cut off the badge is clickable (`isExpandable = clientWidth < scrollWidth`) and a click toggles `isExpanded` (wraps the full text); hovering an expandable badge uses `--mainBackground` | same: click a shortened badge to expand/collapse; tooltip always shows the full label |
| Drawer rows | `.DrawerItem`: grid `minmax(30%, min-content) auto`, `padding: 8px 0`, `border-bottom: 1px solid var(--borderFaintColor)`; name `drawerItemNameColor`, value `drawerItemValueColor` | `details::kv_row` |
| True/False badges | BadgeBoolean: "True" `--colorSuccess`, "False" `--colorError`, otherwise "-" `--colorInfo` (line ~287065) | Nodes "Schedulable" column |

## Pod shell — `dock/terminal/terminal.ts` (line 279731), execShell in `pods/…menu` (line 517460)

| What | Original | In this app |
|---|---|---|
| Command | `kubectl exec -i -t -n <ns> <pod> [-c <container>] -- sh -c "clear; (bash \|\| ash \|\| sh)"`; on a Windows node `-- powershell` | same; Windows when the pod's `nodeSelector` has `kubernetes.io/os=windows`. **Deliberate addition**: `export TERM=xterm-256color; export LANG=${LANG:-C.UTF-8}` before `clear` so colours, line drawing and UTF-8/emoji work in containers without TERM or a locale |
| Tab title | `Pod: <name> (namespace: <ns>)` | same |
| xterm options | `cursorBlink: true`, `cursorStyle: "bar"`, `fontSize` = preference, default **12** (`defaultTerminalConfig`, line 280519), font `RobotoMono` (line 228571), scrollback = xterm.js default **1000** (line 275936) | 12 px, blinking 1.5 px bar, 1000 lines; egui's built-in monospace font |
| Copy | `terminalCopyOnSelect` default **false** (line 452878); key handler: Ctrl+C with a selection copies (xterm), without one goes to the shell; Ctrl+W kept for the shell | selecting doesn't copy; Ctrl+C copies the selection, else sends interrupt |
| Paste | browser paste (Ctrl+V); right-click pastes **only** when copy-on-select is on | Ctrl+V / Ctrl+Shift+V paste (bracketed when asked); right-click does nothing |
| Links | web-links addon: URLs open in the browser (`openLinkInBrowser`) | clicking an http(s) URL opens it; pointer becomes a hand over it |
| Colours | `terminalColors` of the Dark theme (line 526585): bg `#000000`, fg `#ffffff`, cursor `#ffffff`, selection `#264f78`, ANSI 0–15 `#2e3436 #cc0000 #4e9a06 #c4a000 #3465a4 #75507b #06989a #d3d7cf #555753 #ef2929 #8ae234 #fce94f #729fcf #ad7fa8 #34e2e2 #eeeeec` | same; 256-colour cube and true colour as in xterm |
| Emoji | colour emoji (Chromium) | monochrome (egui can't draw colour emoji) |

## Ingresses — `kube-object/src/specifics/ingress.ts` (computeRuleDeclarations line 281580), `network-ingresses/ingress-details.tsx` (line 499590)

| What | Original | In this app |
|---|---|---|
| Link per rule path | protocol `https` when `spec.tls` has **any** entry, else `http`; host defaults to `"*"`, path to `"/"`; `url = protocol://host + path`; shown as a link only when the host has no `*` | `util::ingress_routes` (same rule; my earlier per-host TLS and load-balancer fallback were removed) |
| Backend text | `name:port.number ?? port.name` | same |
| List "Rules" column | `url ⇢ service` per path | same |
| List "LoadBalancers" | `hostname \|\| ip` | same |
| Details | Ports (`80` if any rule has http, else default backend port; `443` if TLS), TLS secret names, per rule "Host: …" + table Path / Link / Backends, then "Hostname / IP" table of load-balancer points | same; links open in the browser. The title-bar 🌐 button and the row-menu "Open …" entries are **additions** the person asked for and use the same link list |

## Volume usage — `storage-volume-claims/volume-claim-disk-chart.tsx` (line 441323), metrics request (line 441279), Prometheus queries in `main.js` 41124

| What | Original | In this app |
|---|---|---|
| Source | Prometheus `sum(kubelet_volume_stats_used_bytes{persistentvolumeclaim,namespace})` and `…capacity_bytes…` | the same kubelet volume statistics read directly from each node (`/api/v1/nodes/<node>/proxy/stats/summary`, `usedBytes`/`capacityBytes` per `pvcRef`), so it works without Prometheus (**addition**: the original shows nothing without Prometheus) |
| Colours | Usage `#ffc63d`, Capacity `chartCapacityColor` `#4c545f` | bar `#ffc63d` on track `#414448` |
| Display | a usage/capacity chart in the claim details | a bar + `NN%  used / capacity` in the claims and volumes lists and in details (**addition** the person asked for) |
| Claim details "Pods" | `volumeClaim.getPods(pods)`: pods whose volumes use the claim | same, each opens the pod |

## Log colours — `dock/logs/log-list.tsx` (line ~524216), ansi_up 6.0.6 (line 523587)

| What | Original | In this app |
|---|---|---|
| Colour codes | every log row goes through `AnsiUp.ansi_to_html` (SGR colours/bold/faint/italic/underline, 256-colour and 24-bit), other escapes removed (`ansiEscapeSequenceRegex`, line 524217) | `ansi.rs`: same SGR set, other CSI/OSC/ESC sequences dropped |
| Palette | ansi_up: `#000000 #bb0000 #00bb00 #bbbb00 #0000bb #bb00bb #00bbbb #ffffff`, bright `#555555 #ff5555 #00ff00 #ffff55 #5555ff #ff55ff #55ffff #ffffff`; 256-colour cube as xterm | same |
| Bold | `font-weight: bold` | egui has no bold monospace: bold text without its own colour is drawn bright white (**approximation**) |
| Rows | `.LogRow`: 18 px high, padding 2 × 16 px, monospace, "smaller" text, hover `logRowHoverBackground` `#35373a`, background `logsBackground` `#000000`, text `logsForeground` `#ffffff` | same (12 px monospace) |
| Search | lines are **not** filtered; matches get `--overlay-bg` `#8cc474b8`, the active match `orange`; next/previous jumps to it | same (↑/↓ buttons or Enter), "n / total" counter |
| Timestamps | optional ("Show timestamps") | toggle; shown dimmed |
| Error/warning hint | — | plain lines without colour codes that mention error/fatal/panic or warn are tinted (**addition**) |
| Colour state across lines | one shared converter, so an unclosed colour can bleed into the next rendered row | each line starts uncoloured (deterministic) |

## Custom resources — `custom-resource-definitions/groups-sidebar-items-computed.injectable.ts` (line 484067), `custom-resources/view.tsx` (line 484512)

| What | Original | In this app |
|---|---|---|
| Loading | definitions are watched from the start of the cluster session | the definition list is loaded on connect and refreshed every 60 s; validation schemas are dropped from the in-memory copy (they were 57 MB on the person's cluster), editing still fetches the full object |
| Menu | Custom Resources → Definitions, then one collapsible entry per API **group** (sorted by group, then plural name), its kinds underneath | same; long group names are shortened with "…" |
| Kind title | `formatResourceKind`: split at `(?<=[a-z])(?=[A-Z])\|(?<=[A-Z])(?=[A-Z][a-z])` → "Service Monitor" | same (`crd::format_kind`) |
| Version used | `getPreferredVersion`: storage version if served, else first served, else storage | same |
| List columns | Name, Namespace (namespaced kinds), `getPrinterColumns(false)` = additionalPrinterColumns without "age" and without `priority`, title `startCase(name)`, Age | same (`crd::register`) |
| Cell values | `formatJSONValue(safeJSONPathValue(obj, jsonPath))`: arrays joined with ", ", objects as JSON, missing → "" | same; JSONPath subset: `.a.b`, `a\.b`, `[n]`, `[*]`, `[?(@.k=="v")]` |
| Details | all printer columns (priority ones too) and status conditions | same |

## Conditions in the details drawer — `kube-object-conditions/kube-object-conditions-drawer.tsx` (line 421133)

| What | Original | In this app |
|---|---|---|
| Order | `sortConditions`: newest `lastTransitionTime` first; ties by priority Synced 6, Ready 5, Initialized 4, ContainersReady / ArtifactInStorage / Released 3, PodReadyToStartContainers 2, PodScheduled 1 (line 421115) | same (`details::conditions_drawer`) |
| Badge | label = `type`; `disabled` (opacity 0.5) when status is False or Unknown; class `error` for those, else `type reason` | same |
| Ready colour | `.PodDetails/.NodeDetails/.CustomResourceDetails .conditions .Ready { background: var(--colorSuccess); color: white }` | Ready + True → `#43a047` background, white text |
| Tooltip | every field of the condition, sorted by key | same, one `key: value` per line |
| Used for | pods, nodes, workloads, custom resources | same |

## Custom resource page and details — `custom-resources/view.tsx` (line 484512), `custom-resources/details.tsx` (line 494071)

| What | Original | In this app |
|---|---|---|
| Page title | `renderHeaderTitle: resource.kind` (e.g. "ServiceMonitor") | same; the sidebar keeps the spaced name |
| Load error | `Failed to load <plural>` | same |
| Details columns | `getPrinterColumns()` = **all** additionalPrinterColumns except "age" (priority ones included) | same |
| Values | `convertSpecValue`: array → list, object → read-only pretty JSON box, boolean → True/False badge, string/number → text | same |
| Then | `KubeObjectConditionsDrawer` | same (see above) |

## Preferences — user-preferences descriptors (bundle line 452795–452843), `main/kubectl/kubectl.ts` (main.js 28669), `kubectl-versions/index.ts` (main.js 29649)

| Setting | Original | In this app |
|---|---|---|
| Download kubectl | `downloadKubectlBinaries`, default **true** | same ("Download kubectl binaries matching the Kubernetes cluster version") |
| Version chosen | cluster `major.minor` looked up in the version map (1.22→1.22.17 … 1.36→1.36.5, 1.37→1.37.1), else the cluster's exact x.y.z | same table (`settings::matching_kubectl`); cluster version from `kubectl version -o json` |
| Download URL | `<mirror>/v<ver>/bin/<os>/<arch>/kubectl`, mirror "Default (Google)" = `https://dl.k8s.io/release` or a custom URL | same |
| Integrity | only versions with a SHA-256 **pinned** in the app (`kubectl-versions/build/checksums.json`, main.js 29155; looked up as `version` + `os/x64|arm64`, kubectl-checksums.injectable 29677) are downloaded; otherwise "No verified checksum is pinned for kubectl vX on os/arch, so it was not downloaded"; a wrong digest gives "Checksum mismatch for kubectl X downloaded from URL: expected sha256 …, got …" (main.js 28980–29050) | same table (113 entries, `src/kubectl_checksums.rs`, generated from the original's JSON) and same messages |
| Stored in | `downloadBinariesPath`/`<ver>/kubectl` | `<binaries>/kubectl/<ver>/kubectl`, default `~/.local/share/cluster-desk/binaries` |
| Fallback | bundled kubectl, or `kubectlBinariesPath` when downloads are off | `kubectl` from PATH, or the configured path |
| Helm | `helmBinariesPath`; bundled Helm `4.3.1` (package.json `bundledHelmVersion`) | path setting + "Download Helm v4.3.1" from get.helm.sh (checked against `.sha256sum`); Helm screens are not built yet |
| HTTP Proxy | `httpsProxy` (no default); placeholder "Type proxy url (example: http://proxy.acme.org:8080)"; http://, https:// or socks5:// | same; used for downloads and passed to kubectl/terminals as `HTTPS_PROXY` |
| Certificate Trust | `allowUntrustedCAs` default **false**: "Allow untrusted Certificate Authorities … Does not affect cluster communications!" | same; applies to the app's own downloads only |
| Terminal | "Terminal copy & paste": `terminalCopyOnSelect` false ("Copy on select and paste on right-click"); "Font size": `terminalConfig.fontSize` 12, input min 10 max 50; also "Terminal Shell Path" and "Font family" | copy & paste and font size (10–50); no local shell or font family choice yet |
| Wording | "Kubectl binary download", "Download mirror", "Custom mirror URL" (placeholder `https://artifacts.example.com/kubernetes/kubectl`), "Directory for binaries" ("The directory to download binaries into."), "Path to kubectl binary" (preferences UI, bundle 462524–462770) | same labels and hints (the path hint says kubectl from PATH, since nothing is bundled) |

**Additions for sign-in (not in the original's preferences)**
- Extra PATH folders for credential plugins (plus `~/.krew/bin`, `~/.local/bin`, `~/go/bin`, `~/bin`, `/usr/local/bin` when present), because desktop-launched apps often get a shorter PATH. (The original's Flatpak bundles kubelogin, gke-gcloud-auth-plugin, aws, doctl instead.)
- Extra kubeconfig files merged into `KUBECONFIG`.
- Per context: how it signs in, read from `kubectl config view -o json` (secrets are redacted by kubectl): exec plugins (OIDC via `kubectl oidc-login`, kubelogin, GKE, EKS, DigitalOcean, …), auth-provider (with a warning that the built-in `oidc` provider was removed in kubectl 1.26), token, client certificate, username/password; whether the plugin program can be found; a **Test** button (`kubectl version`) and **Sign in in a terminal** (runs `kubectl get namespaces` in a real terminal so prompts, device codes and MFA can be answered).
- Settings are saved as JSON in `~/.config/cluster-desk/settings.json`.

## Values settled later (previously estimated)

| What | Original | Where | In this app |
|---|---|---|---|
| Table row height | `rowLineHeight` 17 + 2 × `rowPadding` 8 = **33 px** | `components/table/table.tsx` defaultProps, bundle 307429 | `views::ROW_H` = 33 (header 33) |
| Log lines | first load `tailLines = logLinesToLoad` = **500**, +500 more each time older lines are requested | `dock/logs/store.ts`, bundle 486684–486707 | `kubectl logs --tail=500` |
| Log refresh | `interval(10, loadMore)` while the tab is open | bundle 486713 | auto-refresh every **10 s** |
| Dock height | dock storage default `height: 300` | bundle 280458 | 300 px |

Still different on purpose: the original keeps lists live with Kubernetes **watches**; this app re-lists every 10 s (custom resource definitions every 60 s), and node/volume usage every 30 s.
