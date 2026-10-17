// Fetch once per frame. Reusing a no-store protocol URL in a second <img>
// triggers another native capture, so display a local blob URL instead.
const pendingFrames = new WeakMap<HTMLImageElement, AbortController>();

export async function loadFrame(img: HTMLImageElement, url: string): Promise<boolean> {
  pendingFrames.get(img)?.abort();
  const abort = new AbortController();
  pendingFrames.set(img, abort);
  const timer = setTimeout(() => abort.abort(), 4000);
  let next: string | undefined;
  try {
    // Revalidate with the last frame's ETag so an unchanged surface answers 304
    // (no body), instead of resending identical pixels every poll.
    const headers: Record<string, string> = {};
    const prevEtag = img.dataset.frameEtag;
    if (prevEtag) headers["If-None-Match"] = prevEtag;
    const response = await fetch(url, { signal: abort.signal, cache: "no-store", headers });
    if (response.status === 422) {
      // The driver's typed reason this window has no frame (minimized, off
      // screen, unproven Wayland surface). The tile shows it.
      const reason = (await response.text()).trim();
      if (pendingFrames.get(img) === abort) img.dataset.frameRefusal = reason || "capture refused";
      return false;
    }
    if (response.status === 304) {
      // Pixels unchanged since our last frame: keep what is shown, and the id
      // it carries, so this stays a live (not stale) tile with zero transfer.
      if (abort.signal.aborted || pendingFrames.get(img) !== abort) return false;
      delete img.dataset.frameRefusal;
      return true;
    }
    if (!response.ok || response.status === 204) return false;
    const screenshotId = response.headers?.get("x-screenshot-id") ?? undefined;
    const etag = response.headers?.get("etag") ?? undefined;
    const bytes = await response.blob();
    if (!bytes.size || abort.signal.aborted) return false;
    next = URL.createObjectURL(bytes);
    const probe = new Image();
    probe.src = next;
    await Promise.race([
      probe.decode(),
      new Promise<never>((_, reject) => {
        if (abort.signal.aborted) reject(new Error("frame deadline"));
        else abort.signal.addEventListener("abort", () => reject(new Error("frame deadline")), { once: true });
      }),
    ]);
    if (abort.signal.aborted || pendingFrames.get(img) !== abort) return false;
    const old = img.dataset.frameUrl;
    img.src = next;
    img.dataset.frameUrl = next;
    // The id travels with the pixels it names; a click on this frame sends it.
    if (screenshotId) img.dataset.screenshotId = screenshotId;
    else delete img.dataset.screenshotId;
    // Remember this frame's ETag so the next poll can revalidate it.
    if (etag) img.dataset.frameEtag = etag;
    else delete img.dataset.frameEtag;
    delete img.dataset.frameRefusal;
    next = undefined;
    if (old) URL.revokeObjectURL(old);
    return true;
  } catch {
    return false;
  } finally {
    clearTimeout(timer);
    if (pendingFrames.get(img) === abort) pendingFrames.delete(img);
    if (next) URL.revokeObjectURL(next);
  }
}

export function releaseFrame(img: HTMLImageElement): void {
  const pending = pendingFrames.get(img);
  pendingFrames.delete(img);
  pending?.abort();
  if (img.dataset.frameUrl) URL.revokeObjectURL(img.dataset.frameUrl);
  delete img.dataset.frameUrl;
  delete img.dataset.ok;
  delete img.dataset.frameRefusal;
  delete img.dataset.screenshotId;
  delete img.dataset.frameEtag;
  img.removeAttribute("src");
}
