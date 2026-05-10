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

	const dpr = Math.min(2, typeof window !== "undefined" ? window.devicePixelRatio || 1 : 2);

	const innerW = 280;
	const imgMaxH = 72;
	const textSize = 11;
	const lineLeading = 14;

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
	let drawW = innerW;
	let drawH = drawW / imgRatio;
	if (drawH > imgMaxH) {
		drawH = imgMaxH;
		drawW = drawH * imgRatio;
	}

	const canvas = document.createElement("canvas");
	const ctx = canvas.getContext("2d");
	if (!ctx) return null;

	ctx.font = `${textSize}px ui-sans-serif, system-ui, sans-serif`;

	const layoutW = Math.max(drawW, 140);
	const textMaxW = layoutW;
	const wrapped: string[] = [];
	for (const line of rawLines) {
		wrapped.push(...wrapLine(ctx, line, textMaxW));
	}
	const displayLines = wrapped.length ? wrapped : ["—"];

	const textBlockH = displayLines.length * lineLeading;
	const layoutH = drawH + 10 + textBlockH;

	canvas.width = Math.ceil(layoutW * dpr);
	canvas.height = Math.ceil(layoutH * dpr);
	ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

	ctx.clearRect(0, 0, layoutW, layoutH);

	const imgX = (layoutW - drawW) / 2;
	const imgY = 0;
	ctx.drawImage(imgEl, imgX, imgY, drawW, drawH);

	ctx.fillStyle = "#0f172a";
	ctx.font = `${textSize}px ui-sans-serif, system-ui, sans-serif`;
	ctx.textAlign = "center";
	ctx.textBaseline = "top";
	let ty = imgY + drawH + 12;
	for (const ln of displayLines) {
		ctx.fillText(ln, layoutW / 2, ty);
		ty += lineLeading;
	}

	const dataUrl = canvas.toDataURL("image/png");
	const comma = dataUrl.indexOf(",");
	if (comma < 0) return null;
	return dataUrl.slice(comma + 1);
}
