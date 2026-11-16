const columns = 52;
const rows = 18;
const shades = [.18, .32, .51, .72, .94].map(alpha => `rgba(248, 248, 248, ${alpha})`);
const surface = Array.from({length: columns * rows}, (_, i) => {
  const x = 2 * (i % columns) / (columns - 1) - 1;
  const z = 2 * Math.floor(i / columns) / (rows - 1) - 1;
  return {
    x,
    z,
    crest: Math.pow(1 - .78 * Math.abs(x), 1.2),
    phase: i * 2.399963,
  };
});

export function mountSpeakingScene(element: HTMLCanvasElement | null) {
  const targetContext = element?.getContext('2d');
  if (!element || !targetContext) return;
  const canvas = element, context = targetContext;

  const reduced = matchMedia('(prefers-reduced-motion: reduce)');
  let width = 0, height = 0, time = .28, previous = 0, paintedAt = 0, frame = 0;
  let strength = .7;
  let grid: Path2D, gridInk: CanvasGradient, shadowInk: CanvasGradient;

  function draw() {
    if (!width || !height) return;
    const energy = canvas.dataset.activity;
    const target = energy === 'speaking' ? 1 : energy === 'hearing' ? .82 : .68;
    strength += (target - strength) * .22;
    const size = Math.min(width, height);
    const halfSpan = width * .465, depth = size * .057, rise = size * .108;
    const yaw = -.18 + .1 * Math.sin(time * 1.35);
    const cosine = Math.cos(yaw), sine = Math.sin(yaw);
    const dotSize = Math.max(.58, Math.min(2.3, size / 435));
    const phase = time * 2.75;
    const particles = shades.map(() => new Path2D());

    for (const point of surface) {
      const wave = Math.cos(phase - point.x * 2.5 + point.z * .36) * point.crest * .58;
      const ripple = Math.sin(phase * 2.1 + point.x * 18 + point.z * 6) * .128;
      const scatter = Math.sin(point.phase + phase * 2.6) * .107;
      const elevation = wave + ripple + scatter;
      const near = (point.z + 1) * .5;
      const perspective = .87 + near * .13;
      const x = width * .5 + (point.x * cosine + point.z * sine * .16) * halfSpan * perspective;
      const y = height * .49 + point.z * depth + point.x * depth * yaw - elevation * rise * strength;
      const sparkle = .5 + .5 * Math.cos(point.phase + phase * 1.74);
      const radius = dotSize * (.55 + near * .38 + sparkle * .44);
      const tone = Math.min(shades.length - 1, Math.floor((near * .52 + sparkle * .47) * shades.length));
      const path = particles[tone];
      path.moveTo(x + radius, y);
      path.arc(x, y, radius, 0, 2 * Math.PI);
    }

    context.clearRect(0, 0, width, height);
    context.lineWidth = .8;
    context.strokeStyle = gridInk;
    context.stroke(grid);
    context.save();
    context.translate(width * .5, height * .71);
    context.scale(width * .46, size * .046);
    context.fillStyle = shadowInk;
    context.fillRect(-1, -1, 2, 2);
    context.restore();
    for (let i = 0; i < particles.length; i++) {
      context.fillStyle = shades[i];
      context.fill(particles[i]);
    }
  }

  function tick(now: number) {
    if (previous) time += Math.min((now - previous) / 1000, .09);
    previous = now;
    if (now - paintedAt >= 30) {
      draw();
      paintedAt = now;
    }
    frame = requestAnimationFrame(tick);
  }

  function sync() {
    cancelAnimationFrame(frame);
    previous = 0;
    if (document.hidden || !width || !height) return;
    draw();
    if (!reduced.matches) frame = requestAnimationFrame(tick);
  }

  const resize = new ResizeObserver(([{contentRect: rect}]) => {
    width = rect.width; height = rect.height;
    if (!width || !height) return;
    const density = Math.min(devicePixelRatio, 1.5);
    canvas.width = Math.round(width * density);
    canvas.height = Math.round(height * density);
    context.setTransform(density, 0, 0, density, 0, 0);
    grid = new Path2D();
    const step = width / 16, slope = height * .32;
    for (let i = -6; i <= 22; i++) {
      const x = step * i;
      grid.moveTo(x - width, height * .58);
      grid.lineTo(x + width, height * .58 + slope);
      grid.moveTo(x + width, height * .58);
      grid.lineTo(x - width, height * .58 + slope);
    }
    gridInk = context.createLinearGradient(0, height * .58, 0, height * .9);
    gridInk.addColorStop(0, 'transparent');
    gridInk.addColorStop(.4, '#b5b5b524');
    gridInk.addColorStop(1, 'transparent');
    shadowInk = context.createRadialGradient(0, 0, .08, 0, 0, 1);
    shadowInk.addColorStop(0, '#000a');
    shadowInk.addColorStop(.7, '#0004');
    shadowInk.addColorStop(1, 'transparent');
    sync();
  });
  resize.observe(canvas);
  reduced.addEventListener('change', sync);
  document.addEventListener('visibilitychange', sync);
  return () => {
    cancelAnimationFrame(frame);
    resize.disconnect();
    reduced.removeEventListener('change', sync);
    document.removeEventListener('visibilitychange', sync);
  };
}
