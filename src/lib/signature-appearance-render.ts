/**
 * Genera un PNG del sello visible igual que la vista previa de Certificados
 * (imagen + líneas de texto), para incrustarlo en el PDF firmado.
 */
import type { SigningCertSummary } from "$lib/tauri/pkcs11";
import {
	type SignatureAppearanceState,
	loadSignatureAppearance,
	previewImageSrc,
	resolvePartsToPreviewLines,
} from "$lib/signature-appearance";

/** Corta texto para caber en `maxWidth` (medido con canvas). */
function wrapLine(ctx: CanvasRenderingContext2D, text: string, maxWidth: number): string[] {
	const t = text.trim();
	if (!t) return [];
	const words = t.split(/\s+/);
	const lines: string[] = [];
	let cur = "";
	for (const w of words) {
		const trial = cur ? `${cur} ${w}` : w;
		if (ctx.measureText(trial).width <= maxWidth) {
			cur = trial;
		} else {
			if (cur) lines.push(cur);
			if (ctx.measureText(w).width > maxWidth) {
				let chunk = "";
				for (const ch of w) {
					const next = chunk + ch;
					if (ctx.measureText(next).width <= maxWidth) chunk = next;
					else {
						if (chunk) lines.push(chunk);
						chunk = ch;
					}
				}
				cur = chunk;
			} else {
				cur = w;
			}
		}
	}
	if (cur) lines.push(cur);
	return lines.length ? lines : [t];
}

function loadImage(src: string): Promise<HTMLImageElement> {
	return new Promise((resolve, reject) => {
		const img = new Image();
		img.crossOrigin = "anonymous";
		img.onload = () => resolve(img);
		img.onerror = () => reject(new Error(`No se pudo cargar la imagen del sello`));
		img.src = src;
	});
}

export type RenderSealOptions = {
	/** Si no se pasa, se usa lo guardado en localStorage (pantalla Certificados). */
	appearance?: SignatureAppearanceState;
};

/**
 * PNG en base64 (sin prefijo `data:…`), listo para `signature_seal_png_base64` en la API batch.
 */
export async function renderSignatureSealPngBase64(
	cert: SigningCertSummary | null,
	options?: RenderSealOptions,
): Promise<string | null> {
	if (typeof document === "undefined") return null;

	const appearance = options?.appearance ?? loadSignatureAppearance();
	const imgSrc = previewImageSrc(appearance);
	const rawLines = resolvePartsToPreviewLines(appearance.parts, cert);

	// Usar un factor de escala muy alto (8x) para "supermuestreo".
	// Como la caja es de apenas 120px, 120 * 8 = 960px de ancho final,
	// lo cual garantiza que el texto se vea súper nítido y de alta calidad en el PDF.
	const renderScale = 8;

	const innerW = 280;
	const imgMaxH = 72;
	const textSize = 9;
	const lineLeading = 10.5;

	let imgEl: HTMLImageElement;
	try {
		imgEl = await loadImage(imgSrc);
	} catch {
		try {
			const fallback =
				typeof window !== "undefined"
					? new URL("/signature-default.png", window.location.origin).href
					: imgSrc;
			imgEl = await loadImage(fallback);
		} catch {
			imgEl = await loadImage(
				"data:image/svg+xml," +
					encodeURIComponent(
						`<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80"><rect fill="#e8eef5" width="120" height="80" rx="8"/><text x="60" y="44" text-anchor="middle" fill="#334155" font-family="system-ui,sans-serif" font-size="12">Firma</text></svg>`,
					),
			);
		}
	}

	const imgRatio = imgEl.naturalWidth / Math.max(1, imgEl.naturalHeight);
	const layoutW = 120;
	const drawW = layoutW;
	const drawH = drawW / imgRatio;

	const canvas = document.createElement("canvas");
	const ctx = canvas.getContext("2d");
	if (!ctx) return null;

	ctx.font = `${textSize}px ui-sans-serif, system-ui, sans-serif`;

	const textMaxW = layoutW;
	const wrapped: { text: string; justify: boolean }[] = [];
	for (const line of rawLines) {
		const lines = wrapLine(ctx, line, textMaxW);
		for (let i = 0; i < lines.length; i++) {
			wrapped.push({ text: lines[i], justify: i < lines.length - 1 });
		}
	}
	const displayLines = wrapped.length ? wrapped : [{ text: "—", justify: false }];

	const textBlockH = displayLines.length * lineLeading;
	const layoutH = drawH + 4 + textBlockH;

	canvas.width = Math.ceil(layoutW * renderScale);
	canvas.height = Math.ceil(layoutH * renderScale);
	ctx.setTransform(renderScale, 0, 0, renderScale, 0, 0);
	ctx.imageSmoothingQuality = "high";

	ctx.clearRect(0, 0, layoutW, layoutH);

	const imgX = (layoutW - drawW) / 2;
	const imgY = 0;
	ctx.drawImage(imgEl, imgX, imgY, drawW, drawH);

	ctx.fillStyle = "#0f172a";
	ctx.font = `${textSize}px ui-sans-serif, system-ui, sans-serif`;
	ctx.textBaseline = "top";
	let ty = imgY + drawH + 4;
	for (const ln of displayLines) {
		if (ln.justify && ln.text.includes(" ")) {
			ctx.textAlign = "left";
			const words = ln.text.split(" ");
			const textWidth = ctx.measureText(ln.text.replace(/\s/g, "")).width;
			const totalSpace = layoutW - textWidth;
			const spacePerWord = totalSpace / (words.length - 1);
			let cx = 0;
			for (let i = 0; i < words.length; i++) {
				ctx.fillText(words[i], cx, ty);
				cx += ctx.measureText(words[i]).width + spacePerWord;
			}
		} else {
			ctx.textAlign = "left";
			ctx.fillText(ln.text, 0, ty);
		}
		ty += lineLeading;
	}

	const dataUrl = canvas.toDataURL("image/png");
	const comma = dataUrl.indexOf(",");
	if (comma < 0) return null;
	return dataUrl.slice(comma + 1);
}
