# Preferences

Open them with the gear at the bottom of the hotbar. Five tabs: [Proxy](#proxy),
[Kubernetes](#kubernetes), [Authentication](#authentication), [Terminal](#terminal), [Updates](#updates).

> [!NOTE]
> Settings are saved as JSON in `~/.config/cluster-desk/settings.json` (`$XDG_CONFIG_HOME/cluster-desk/`
> when set; on Windows, under your user profile folder). The exact path is shown at the top of the
> Preferences screen.

## Proxy

| Setting | Default | What it does |
|---|---|---|
| HTTP Proxy | empty | Proxy for the app's own downloads and Helm. Takes `http://`, `https://` or `socks5://` URLs. Also passed to kubectl and terminals as `HTTPS_PROXY`. |
| Allow untrusted Certificate Authorities | off | Trusts any certificate for the app's own downloads. Needed behind some corporate proxies that rewrite certificates. **Never** affects connections to your clusters. |

## Kubernetes

### Kubectl binary download

| Setting | Default | What it does |
|---|---|---|
| Download kubectl binaries matching the Kubernetes cluster version | on | Each cluster gets the kubectl release made for its version, downloaded once and checked against a checksum built into the app. Versions without a pinned checksum are not downloaded. |
| Download mirror | Default (Google) | `https://dl.k8s.io/release`, or *Custom* with your own base URL. The version and platform path are added to it. |
| Directory for binaries | `~/.local/share/cluster-desk/binaries` | Where downloaded tools are kept. |
| Path to kubectl binary | kubectl on `PATH` | Used when downloads are off. |

*Prepare kubectl for the open cluster* downloads the matching kubectl right away instead of on first use.

### Helm

*Path to helm binary* (default: helm on `PATH`), or *Download Helm* to fetch the pinned version.
No screen uses Helm yet.

### Kubeconfig files

`KUBECONFIG` (or `~/.kube/config`) is always read. Add more files with *Add file*; they are merged in.
*Reload clusters* re-reads them all.

## Authentication

### Where sign-in programs are found

Clusters that sign in through a plugin need that program on the `PATH`. Apps started from a desktop
launcher often get a shorter `PATH` than your terminal, so add the folders here with *Add folder*.

Searched automatically when present: `~/.krew/bin`, `~/.local/bin`, `~/go/bin`, `~/bin`, `/usr/local/bin`.

### Clusters and how they sign in

One row per kubeconfig context: its server, its user, how that user signs in, and whether the sign-in
program was found.

| Detected method | From the kubeconfig user |
|---|---|
| OIDC (kubelogin plugin) | `exec: kubectl oidc-login …`. Opens your browser the first time; the token is cached. |
| kubelogin (Azure / OIDC) | `exec: kubelogin …`. May open your browser or show a device code. |
| Google Cloud | `exec: gke-gcloud-auth-plugin` |
| AWS (EKS) | `exec: aws` or `aws-iam-authenticator` |
| DigitalOcean, Oracle Cloud, Teleport | `exec: doctl`, `oci`, `tsh` |
| Bearer token, client certificate, username and password | Static credentials in the kubeconfig. |

Two buttons per cluster:

- **Test** connects and shows the Kubernetes version, or the error.
- **Sign in in a terminal** runs kubectl for that cluster in a terminal, so you can answer sign-in
  prompts and device codes. Use it when a plugin is marked as interactive.

> [!NOTE]
> The built-in `oidc` auth-provider was removed in kubectl 1.26. Switch such users to the
> kubelogin exec plugin (`kubectl oidc-login`).

## Terminal

| Setting | Default | What it does |
|---|---|---|
| Copy on select and paste on right-click | off | Selecting text copies it; right-click pastes. |
| Font size | 12 | Font size of pod shells. |

## Updates

| Setting | Default | What it does |
|---|---|---|
| Check for updates automatically | on | Every 6 hours, look for a newer release, download it and check its signature. Installs when you quit. |

The tab shows the running version, the update state, *Check now*, and *Restart to update* once an
update is downloaded. Details: [Install → Updates](install.md#updates).
