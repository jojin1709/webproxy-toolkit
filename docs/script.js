const OWNER = "jojin1709";
const REPO = "webproxy-toolkit";
const API = `https://api.github.com/repos/${OWNER}/${REPO}/releases/latest`;

function pickAsset(release) {
  if (!release.assets || !release.assets.length) return null;
  // Prefer the Windows NSIS installer, fall back to first .exe
  return (
    release.assets.find((a) => /setup\.exe$/i.test(a.name) && /x64/i.test(a.name)) ||
    release.assets.find((a) => /\.exe$/i.test(a.name)) ||
    release.assets[0]
  );
}

function renderNotes(body) {
  if (!body) return "<p class='muted'>No release notes for this version.</p>";
  // Keep it light: render markdown-ish bullets and headings safely
  const esc = (s) =>
    s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const lines = body.split("\n");
  let html = "<ul>";
  let inList = false;
  for (const raw of lines) {
    const line = raw.trim();
    if (!line) continue;
    if (/^#{1,6}\s+/.test(line)) {
      if (inList) { html += "</ul>"; inList = false; }
      html += `<p><strong>${esc(line.replace(/^#{1,6}\s+/, ""))}</strong></p>`;
    } else if (/^[-*]\s+/.test(line)) {
      if (!inList) { html += "<ul>"; inList = true; }
      html += `<li>${esc(line.replace(/^[-*]\s+/, ""))}</li>`;
    } else {
      if (inList) { html += "</ul>"; inList = false; }
      html += `<p>${esc(line)}</p>`;
    }
  }
  if (inList) html += "</ul>";
  return html;
}

async function loadRelease() {
  const heroBtn = document.getElementById("hero-download");
  const heroLabel = document.getElementById("hero-download-label");
  const heroVersion = document.getElementById("hero-version");
  const dlButton = document.getElementById("dl-button");
  const dlVersion = document.getElementById("dl-version");
  const dlFile = document.getElementById("dl-file");
  const dlNotes = document.getElementById("dl-notes");
  const dlAll = document.getElementById("dl-all");
  const wnTitle = document.getElementById("wn-title");
  const wnDate = document.getElementById("wn-date");
  const wnNotes = document.getElementById("wn-notes");
  const wnBadge = document.getElementById("wn-badge");

  try {
    const res = await fetch(API, { headers: { Accept: "application/vnd.github+json" } });
    if (!res.ok) throw new Error(`GitHub API ${res.status}`);
    const release = await res.json();
    const asset = pickAsset(release);

    // Version + hero
    const tag = release.tag_name || "latest";
    heroVersion.textContent = `${tag} · updated ${new Date(release.published_at).toLocaleDateString()}`;

    if (asset) {
      heroBtn.href = asset.browser_download_url;
      heroBtn.removeAttribute("aria-disabled");
      heroLabel.textContent = `Download ${tag} for Windows`;

      dlButton.href = asset.browser_download_url;
      dlButton.removeAttribute("aria-disabled");
      dlVersion.textContent = tag;
      const mb = (asset.size / (1024 * 1024)).toFixed(1);
      dlFile.textContent = `${asset.name} · ${mb} MB · Windows x64`;
    } else {
      dlAll.focus();
      heroVersion.textContent = `${tag} · no binary attached yet — see releases`;
      heroBtn.href = release.html_url;
      heroBtn.removeAttribute("aria-disabled");
      heroLabel.textContent = "View release on GitHub";
      dlVersion.textContent = tag;
      dlFile.textContent = "No installer attached to this release yet.";
    }

    // What's new panel
    wnTitle.textContent = release.name || tag;
    wnTitle.href = release.html_url;
    wnDate.textContent = new Date(release.published_at).toLocaleString();
    if (wnBadge) wnBadge.textContent = tag;

    const notes = renderNotes(release.body);
    wnNotes.innerHTML = notes;

    // Compact bullet list on download card
    const firstItems = (release.body || "")
      .split("\n")
      .map((l) => l.trim())
      .filter((l) => /^[-*]\s+/.test(l))
      .slice(0, 4)
      .map((l) => `<li>${l.replace(/^[-*]\s+/, "")}</li>`);
    dlNotes.innerHTML = firstItems.length
      ? firstItems.join("")
      : "<li>Latest release of WebProxy Toolkit.</li>";
  } catch (err) {
    heroVersion.textContent = "Could not fetch release — view it on GitHub.";
    wnNotes.innerHTML =
      "<p class='muted'>Could not load release notes. <a href='https://github.com/" +
      OWNER +
      "/" +
      REPO +
      "/releases'>View releases on GitHub</a>.</p>";
    dlFile.textContent = "See GitHub releases for the installer.";
  }
}

document.addEventListener("DOMContentLoaded", loadRelease);
