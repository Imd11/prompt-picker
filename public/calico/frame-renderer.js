function idempotentRelease(release) {
  var released = false;
  return function () {
    if (released) return;
    released = true;
    release();
  };
}

export async function loadImageBitmapSurface(file, options) {
  options = options || {};
  var fetchFn = options.fetchFn || globalThis.fetch.bind(globalThis);
  var createBitmap = options.createImageBitmapFn || globalThis.createImageBitmap.bind(globalThis);
  var response = await fetchFn(file, { cache: "force-cache", signal: options.signal });
  if (!response.ok) throw new Error("Failed to fetch Calico sheet: " + response.status);
  var bitmap = await createBitmap(await response.blob());
  if (typeof bitmap.close !== "function") {
    if (typeof bitmap.close === "function") bitmap.close();
    throw new Error("ImageBitmap.close is unavailable");
  }
  return {
    source: bitmap,
    backend: "image-bitmap",
    release: idempotentRelease(function () { bitmap.close(); }),
  };
}

export async function loadHtmlImageSurface(file, options) {
  options = options || {};
  var fetchFn = options.fetchFn || globalThis.fetch.bind(globalThis);
  var urlApi = options.urlApi || globalThis.URL;
  var imageFactory = options.imageFactory || function () { return new Image(); };
  var response = await fetchFn(file, { cache: "force-cache", signal: options.signal });
  if (!response.ok) throw new Error("Failed to fetch Calico sheet: " + response.status);
  var objectUrl = urlApi.createObjectURL(await response.blob());
  var image = imageFactory();
  var release = idempotentRelease(function () {
    image.src = "";
    urlApi.revokeObjectURL(objectUrl);
  });
  try {
    image.src = objectUrl;
    if (typeof image.decode === "function") {
      await image.decode();
    } else {
      await new Promise(function (resolve, reject) {
        image.onload = resolve;
        image.onerror = function () { reject(new Error("Calico sheet image load failed")); };
      });
    }
    return { source: image, backend: "html-image", release: release };
  } catch (error) {
    release();
    throw error;
  }
}

export function calculateContainRect(sourceWidth, sourceHeight, targetWidth, targetHeight) {
  var scale = Math.min(targetWidth / sourceWidth, targetHeight / sourceHeight);
  var width = sourceWidth * scale;
  var height = sourceHeight * scale;
  return {
    x: (targetWidth - width) / 2,
    y: (targetHeight - height) / 2,
    width: width,
    height: height,
  };
}

export function frameGeometry(sheet, frameIndex, targetWidth, targetHeight) {
  var column = frameIndex % sheet.columns;
  var row = Math.floor(frameIndex / sheet.columns);
  return {
    sourceX: column * sheet.strideX,
    sourceY: row * sheet.strideY,
    sourceWidth: sheet.frameWidth,
    sourceHeight: sheet.frameHeight,
    destination: calculateContainRect(
      sheet.frameWidth,
      sheet.frameHeight,
      targetWidth,
      targetHeight
    ),
  };
}

export function playbackFrameAt(sheet, elapsedMs) {
  var durations = sheet.frameDurationsMs;
  var cycleDuration = durations.reduce(function (sum, value) { return sum + value; }, 0);
  var finiteDuration = sheet.plays > 0 ? cycleDuration * sheet.plays : Infinity;
  if (sheet.plays > 0 && elapsedMs >= finiteDuration) {
    return { frameIndex: sheet.frameCount - 1, done: true, nextDelayMs: null };
  }
  var inCycle = elapsedMs % cycleDuration;
  var boundary = 0;
  for (var index = 0; index < durations.length; index += 1) {
    boundary += durations[index];
    if (inCycle < boundary) {
      return {
        frameIndex: index,
        done: false,
        nextDelayMs: Math.max(1, boundary - inCycle),
      };
    }
  }
  return { frameIndex: sheet.frameCount - 1, done: false, nextDelayMs: 1 };
}

function drawSourceFrame(context, source, sheet, frameIndex, width, height) {
  var geometry = frameGeometry(sheet, frameIndex, width, height);
  var destination = geometry.destination;
  context.setTransform(1, 0, 0, 1, 0, 0);
  context.clearRect(0, 0, width, height);
  context.drawImage(
    source,
    geometry.sourceX,
    geometry.sourceY,
    geometry.sourceWidth,
    geometry.sourceHeight,
    destination.x,
    destination.y,
    destination.width,
    destination.height
  );
}

function createDefaultSurfaceLoader(scratchCanvas) {
  var selectedBackend = "";
  var warned = false;
  return async function (file, options) {
    if (selectedBackend === "image-bitmap") return loadImageBitmapSurface(file, options);
    if (selectedBackend === "html-image") return loadHtmlImageSurface(file, options);
    if (typeof globalThis.createImageBitmap === "function") {
      var preferred = null;
      try {
        preferred = await loadImageBitmapSurface(file, options);
        var context = scratchCanvas.getContext("2d", { alpha: true });
        context.drawImage(preferred.source, 0, 0, 1, 1);
        context.clearRect(0, 0, 1, 1);
        selectedBackend = "image-bitmap";
        return preferred;
      } catch (error) {
        if (preferred) preferred.release();
        if (!warned) {
          warned = true;
          console.warn("Calico ImageBitmap backend unavailable; using HTMLImageElement.", error);
        }
      }
    }
    selectedBackend = "html-image";
    return loadHtmlImageSurface(file, options);
  };
}

export function createCalicoFrameRenderer(options) {
  options = options || {};
  var canvas = options.canvas;
  if (!canvas) throw new Error("Calico renderer requires one canvas");
  var presentationElement = options.presentationElement || canvas;
  var createCanvas = options.createCanvas || function () { return document.createElement("canvas"); };
  var scratch = createCanvas();
  var visibleContext = canvas.getContext("2d", { alpha: true });
  var scratchContext = scratch.getContext("2d", { alpha: true });
  if (!visibleContext || !scratchContext) throw new Error("Calico renderer requires 2D canvas contexts");

  var maxDecodedSheets = options.maxDecodedSheets || 2;
  var loadSurface = options.loadSurface || createDefaultSurfaceLoader(scratch);
  var loadBaseline = options.loadBaseline || loadHtmlImageSurface;
  var drawFrame = options.drawFrame || drawSourceFrame;
  var setTimer = options.setTimer || globalThis.setTimeout.bind(globalThis);
  var clearTimer = options.clearTimer || globalThis.clearTimeout.bind(globalThis);
  var now = options.now || function () { return performance.now(); };
  var onError = options.onError || console.error.bind(console);
  var onFatalRender = options.onFatalRender || onError;
  var assetVersion = options.assetVersion || "1";

  var generation = 0;
  var scratchVersion = 0;
  var frameTimer = 0;
  var pendingDecode = null;
  var queuedRequest = null;
  var decoded = new Map();
  var lruCounter = 0;
  var active = null;
  var baselineSurface = null;
  var state = "initializing";
  var visualReady = false;
  var fatalReported = false;
  var suspendedElapsed = 0;
  var backend = "";
  var staleGenerationDrawCount = 0;

  function ensureScratchSize(width, height) {
    if (scratch.width !== width) scratch.width = width;
    if (scratch.height !== height) scratch.height = height;
    scratchContext = scratch.getContext("2d", { alpha: true });
  }

  function clearFrameTimer() {
    if (!frameTimer) return;
    clearTimer(frameTimer);
    frameTimer = 0;
  }

  function failFatal(error) {
    clearFrameTimer();
    state = "lost";
    visualReady = false;
    if (!fatalReported) {
      fatalReported = true;
      onFatalRender(error);
    }
  }

  function renderToScratch(source, sheet, frameIndex, width, height) {
    ensureScratchSize(width, height);
    drawFrame(scratchContext, source, sheet, frameIndex, width, height);
    scratchVersion += 1;
  }

  function commitScratch() {
    try {
      visibleContext.save();
      visibleContext.setTransform(1, 0, 0, 1, 0, 0);
      visibleContext.globalCompositeOperation = "copy";
      visibleContext.drawImage(scratch, 0, 0);
      visibleContext.restore();
      fatalReported = false;
      visualReady = true;
      if (state !== "suspended") state = "ready";
      return true;
    } catch (error) {
      try { visibleContext.restore(); } catch (_) {}
      failFatal(error);
      return false;
    }
  }

  function renderCandidate(candidate, frameIndex, targetWidth, targetHeight) {
    var width = targetWidth || canvas.width;
    var height = targetHeight || canvas.height;
    try {
      renderToScratch(candidate.surface.source, candidate.sheet, frameIndex, width, height);
    } catch (error) {
      onError(error);
      return false;
    }
    return commitScratch();
  }

  function scheduleCurrentFrame(requestGeneration) {
    clearFrameTimer();
    if (!active || state !== "ready" || active.baseline) return;
    var playback = playbackFrameAt(active.sheet, Math.max(0, now() - active.startedAt));
    if (!renderCandidate(active, playback.frameIndex)) return;
    active.frameIndex = playback.frameIndex;
    if (playback.done || requestGeneration !== generation) return;
    frameTimer = setTimer(function () {
      frameTimer = 0;
      if (requestGeneration !== generation || !active) return;
      scheduleCurrentFrame(requestGeneration);
    }, playback.nextDelayMs);
  }

  function releaseEntry(file) {
    var entry = decoded.get(file);
    if (!entry) return;
    decoded.delete(file);
    entry.surface.release();
  }

  function releaseBaselineSurface() {
    if (!baselineSurface) return;
    baselineSurface.release();
    baselineSurface = null;
  }

  function liveSurfaceCount() {
    return decoded.size + (baselineSurface ? 1 : 0);
  }

  function evictOldestInactiveEntry() {
    var candidateFile = "";
    var candidateStamp = Infinity;
    decoded.forEach(function (entry, file) {
      if (active && !active.baseline && active.file === file) return;
      if (entry.stamp < candidateStamp) {
        candidateFile = file;
        candidateStamp = entry.stamp;
      }
    });
    if (!candidateFile) return false;
    releaseEntry(candidateFile);
    return true;
  }

  function reserveIncomingSurface() {
    while (liveSurfaceCount() >= maxDecodedSheets) {
      if (!evictOldestInactiveEntry()) return false;
    }
    return true;
  }

  function trimSurfaceBudget() {
    while (liveSurfaceCount() > maxDecodedSheets && evictOldestInactiveEntry()) {}
  }

  function activateRequest(request, entry) {
    var candidate = {
      state: request.state,
      file: request.sheet.file,
      sheet: request.sheet,
      surface: entry.surface,
      startedAt: now(),
      frameIndex: 0,
      baseline: false,
    };
    if (!renderCandidate(candidate, 0)) {
      releaseEntry(candidate.file);
      request.resolve(false);
      return;
    }
    active = candidate;
    releaseBaselineSurface();
    entry.stamp = ++lruCounter;
    state = "ready";
    clearFrameTimer();
    scheduleCurrentFrame(request.generation);
    request.resolve(true);
  }

  function pump() {
    if (pendingDecode || !queuedRequest || state === "disposed" || state === "suspended") return;
    var request = queuedRequest;
    queuedRequest = null;
    var cached = decoded.get(request.sheet.file);
    if (cached) {
      activateRequest(request, cached);
      pump();
      return;
    }

    if (!reserveIncomingSurface()) {
      request.resolve(false);
      pump();
      return;
    }
    var controller = typeof AbortController === "function" ? new AbortController() : null;
    pendingDecode = { request: request, controller: controller };
    loadSurface(request.sheet.file, { signal: controller ? controller.signal : undefined })
      .then(function (surface) {
        backend = surface.backend || backend || "injected";
        if (request.generation !== generation || state === "disposed" || state === "suspended") {
          surface.release();
          request.resolve(false);
          return;
        }
        decoded.set(request.sheet.file, { surface: surface, stamp: ++lruCounter });
        activateRequest(request, decoded.get(request.sheet.file));
      })
      .catch(function (error) {
        if (!controller || !controller.signal.aborted) onError(error);
        request.resolve(false);
      })
      .finally(function () {
        pendingDecode = null;
        pump();
      });
  }

  function queue(request) {
    if (queuedRequest) queuedRequest.resolve(false);
    queuedRequest = request;
    if (pendingDecode && pendingDecode.request.generation !== request.generation
        && pendingDecode.controller) {
      pendingDecode.controller.abort();
    }
    pump();
  }

  function play(stateName, sheet, playbackOptions) {
    playbackOptions = playbackOptions || {};
    if (state === "disposed") return Promise.resolve(false);
    generation += 1;
    var requestGeneration = generation;
    if (active && active.file === sheet.file) {
      active.state = stateName;
      active.sheet = sheet;
      if (playbackOptions.restart) active.startedAt = now();
      state = "ready";
      scheduleCurrentFrame(requestGeneration);
      return Promise.resolve(true);
    }
    return new Promise(function (resolve) {
      queue({
        generation: requestGeneration,
        state: stateName,
        sheet: sheet,
        resolve: resolve,
      });
    });
  }

  async function showBaseline() {
    if (state === "disposed") return false;
    generation += 1;
    var requestGeneration = generation;
    clearFrameTimer();
    if (!baselineSurface) {
      if (!reserveIncomingSurface()) return false;
      var loadedBaseline = null;
      try {
        loadedBaseline = await loadBaseline(
          "/calico/calico-idle-follow.svg?v=" + encodeURIComponent(assetVersion),
          {}
        );
      } catch (error) {
        onError(error);
        visualReady = false;
        return false;
      }
      if (requestGeneration !== generation) {
        loadedBaseline.release();
        return false;
      }
      baselineSurface = loadedBaseline;
    }
    if (requestGeneration !== generation) return false;
    var source = baselineSurface.source;
    var width = source.naturalWidth || source.width || 266;
    var height = source.naturalHeight || source.height || 200;
    var baselineSheet = {
      frameWidth: width,
      frameHeight: height,
      frameCount: 1,
      columns: 1,
      strideX: width,
      strideY: height,
      frameDurationsMs: [1],
      plays: 1,
    };
    var candidate = {
      state: "idle-follow",
      file: "",
      sheet: baselineSheet,
      surface: baselineSurface,
      startedAt: now(),
      frameIndex: 0,
      baseline: true,
    };
    if (!renderCandidate(candidate, 0)) return false;
    active = candidate;
    trimSurfaceBudget();
    state = "ready";
    return true;
  }

  function setPresentation(presentation) {
    presentation = presentation || {};
    presentationElement.style.setProperty("--calico-scale", String(presentation.scale || 1));
    presentationElement.style.setProperty("--calico-offset-x", String(presentation.offsetX || 0) + "px");
    presentationElement.style.setProperty("--calico-offset-y", String(presentation.offsetY || 0) + "px");
  }

  function redrawCurrentFrame() {
    if (!active || state === "disposed") return false;
    return renderCandidate(active, active.frameIndex);
  }

  function suspend(suspendOptions) {
    suspendOptions = suspendOptions || {};
    generation += 1;
    clearFrameTimer();
    if (pendingDecode && pendingDecode.controller) pendingDecode.controller.abort();
    if (queuedRequest) queuedRequest.resolve(false);
    queuedRequest = null;
    suspendedElapsed = active ? Math.max(0, now() - active.startedAt) : 0;
    Array.from(decoded.keys()).forEach(function (file) {
      if (!active || file !== active.file) releaseEntry(file);
    });
    state = "suspended";
    if (!suspendOptions.retainFrame) visualReady = false;
  }

  function resume() {
    if (state !== "suspended") return false;
    visibleContext = canvas.getContext("2d", { alpha: true });
    if (!visibleContext) {
      failFatal(new Error("Calico canvas context unavailable after resume"));
      return false;
    }
    if (active) active.startedAt = now() - suspendedElapsed;
    state = "ready";
    if (!redrawCurrentFrame()) return false;
    scheduleCurrentFrame(generation);
    return true;
  }

  function prepareBackingStoreResize(nextDpr) {
    if (!active || state === "lost" || state === "disposed") return null;
    var cssWidth = canvas.clientWidth || Number.parseFloat(canvas.style.width) || canvas.width;
    var cssHeight = canvas.clientHeight || Number.parseFloat(canvas.style.height) || canvas.height;
    var width = Math.max(1, Math.round(cssWidth * nextDpr));
    var height = Math.max(1, Math.round(cssHeight * nextDpr));
    try {
      renderToScratch(active.surface.source, active.sheet, active.frameIndex, width, height);
    } catch (error) {
      onError(error);
      return null;
    }
    return {
      generation: generation,
      scratchVersion: scratchVersion,
      width: width,
      height: height,
    };
  }

  function commitPreparedResize(token) {
    if (!token || token.generation !== generation || token.scratchVersion !== scratchVersion
        || state === "lost" || state === "disposed") return false;
    canvas.width = token.width;
    canvas.height = token.height;
    visibleContext = canvas.getContext("2d", { alpha: true });
    if (!visibleContext) {
      failFatal(new Error("Calico canvas context unavailable during resize"));
      return false;
    }
    return commitScratch();
  }

  function dispose() {
    if (state === "disposed") return;
    generation += 1;
    clearFrameTimer();
    if (pendingDecode && pendingDecode.controller) pendingDecode.controller.abort();
    pendingDecode = null;
    if (queuedRequest) queuedRequest.resolve(false);
    queuedRequest = null;
    decoded.forEach(function (entry) { entry.surface.release(); });
    decoded.clear();
    releaseBaselineSurface();
    active = null;
    visualReady = false;
    state = "disposed";
    visibleContext.clearRect(0, 0, canvas.width, canvas.height);
  }

  function diagnostics() {
    return {
      backend: backend,
      state: state,
      decodedSheetCount: decoded.size,
      liveSurfaceCount: liveSurfaceCount(),
      pendingDecodeCount: pendingDecode ? 1 : 0,
      queuedRequestCount: queuedRequest ? 1 : 0,
      activeTimerCount: frameTimer ? 1 : 0,
      staleGenerationDrawCount: staleGenerationDrawCount,
      visualReady: visualReady,
    };
  }

  canvas.addEventListener("contextlost", function (event) {
    if (event && typeof event.preventDefault === "function") event.preventDefault();
    failFatal(new Error("Calico canvas context lost"));
  });

  return {
    play: play,
    showBaseline: showBaseline,
    setPresentation: setPresentation,
    redrawCurrentFrame: redrawCurrentFrame,
    suspend: suspend,
    resume: resume,
    prepareBackingStoreResize: prepareBackingStoreResize,
    commitPreparedResize: commitPreparedResize,
    dispose: dispose,
    diagnostics: diagnostics,
  };
}
