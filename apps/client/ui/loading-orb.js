const fullTurn = Math.PI * 2;
const elevation = .49;
const elevationCos = Math.cos(elevation);
const elevationSin = Math.sin(elevation);

function lattice(count) {
  const last = count - 1;
  const edge = index => index === 0 ? -1 : index === last ? 1 : 0;
  return Array.from({ length: count }, (_, layer) => {
    const y = 2 * layer / last - 1;
    const normalY = edge(layer);
    const dots = [];
    for (let column = 0; column < count; column++) {
      const x = 2 * column / last - 1;
      const normalX = edge(column);
      for (let row = 0; row < count; row++) {
        const normalZ = edge(row);
        if (normalX || normalY || normalZ) {
          dots.push({ x, z: 2 * row / last - 1, normalX, normalZ });
        }
      }
    }
    return { y, normalY, dots };
  });
}

const fullLattice = lattice(9);
const smallLattice = lattice(4);

export function mountLoadingOrb(canvas, compact = false) {
  const context = canvas.getContext('2d');
  if (!context) return { setRunning() {}, dispose() {} };
  const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)');
  const layers = compact ? smallLattice : fullLattice;
  let size = 0, frame = 0, previousTime = 0, paintedAt = 0, elapsed = 0;
  let visible = false, running = false;
  let ink;

  function paint(time) {
    const centerX = size * .5;
    const centerY = size * (compact ? .5 : .455);
    const unit = size * (compact ? .25 : .235);
    const yaw = -.44 + time * .91;
    const foldPhase = time * 3.7 - 1.95;
    const pulse = Math.cos(foldPhase);
    const forwardFold = ((1 + pulse) / 2) ** 6;
    const returnFold = ((1 - pulse) / 2) ** 7;
    const torsion = .43 * forwardFold - .29 * returnFold;
    const seam = -.06 + .4 * returnFold;
    context.clearRect(0, 0, size, size);

    if (!compact) {
      const shadowRadius = unit * 1.51;
      context.save();
      context.translate(centerX, size * .875);
      context.scale(1, .25);
      const shadow = context.createRadialGradient(0, 0, 0, 0, 0, shadowRadius);
      shadow.addColorStop(0, 'rgba(0, 0, 0, .15)');
      shadow.addColorStop(1, 'transparent');
      context.fillStyle = shadow;
      context.fillRect(-shadowRadius, -shadowRadius, shadowRadius * 2, shadowRadius * 2);
      context.restore();
    }

    context.fillStyle = ink;
    for (const { y, normalY, dots } of layers) {
      const angle = yaw + torsion * Math.tanh((y - seam) * 4);
      const cosine = Math.cos(angle), sine = Math.sin(angle);
      const viewX = -sine * elevationCos, viewZ = cosine * elevationCos;
      const facingY = normalY * elevationSin;
      for (const { x, z, normalX, normalZ } of dots) {
        const facing = Math.max(normalX * viewX, normalZ * viewZ, facingY);
        if (facing <= 0) continue;
        const depth = z * cosine - x * sine;
        const dotX = centerX + unit * (x * cosine + z * sine);
        const dotY = centerY + unit * (depth * elevationSin - y * elevationCos);
        const light = .66 + (depth * elevationCos + y * elevationSin) * .17;
        const dotRadius = size * (compact ? .043 : .0051 + .0043 * light);
        context.globalAlpha = (.35 + .65 * light) * Math.min(1, facing * 3.15);
        context.beginPath();
        context.arc(dotX, dotY, dotRadius, 0, fullTurn);
        context.fill();
      }
    }
    context.globalAlpha = 1;
  }

  function tick(now) {
    if (previousTime) elapsed += Math.min(.1, (now - previousTime) / 1000);
    previousTime = now;
    if (now - paintedAt >= 1000 / (compact ? 24 : 30)) {
      paint(elapsed);
      paintedAt = now;
    }
    frame = requestAnimationFrame(tick);
  }

  function sync() {
    cancelAnimationFrame(frame);
    previousTime = 0;
    if (!visible || !size || document.hidden) return;
    ink = getComputedStyle(canvas).color;
    paint(reducedMotion.matches ? 0 : elapsed);
    if (running && !reducedMotion.matches) frame = requestAnimationFrame(tick);
  }

  const geometry = new ResizeObserver(([entry]) => {
    size = entry.contentRect.width;
    const density = Math.min(devicePixelRatio, 2);
    canvas.width = canvas.height = Math.round(size * density);
    context.setTransform(density, 0, 0, density, 0, 0);
    sync();
  });
  geometry.observe(canvas);
  const visibility = new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting;
    sync();
  });
  visibility.observe(canvas);
  reducedMotion.addEventListener('change', sync);
  document.addEventListener('visibilitychange', sync);

  return {
    setRunning(next) {
      if (running === next) return;
      running = next;
      sync();
    },
    dispose() {
      geometry.disconnect();
      visibility.disconnect();
      cancelAnimationFrame(frame);
      reducedMotion.removeEventListener('change', sync);
      document.removeEventListener('visibilitychange', sync);
    },
  };
}
