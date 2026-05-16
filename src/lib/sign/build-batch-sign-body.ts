import type { BatchSignBody } from "$lib/api/local-api";
import type { SigningCertSummary } from "$lib/tauri/pkcs11";
import { renderSignatureSealPngBase64 } from "$lib/signature-appearance-render";

export async function buildBatchSignBodyFromWizard(opts: {
	certIdHex: string;
	paths: string[];
	pin: string;
	sigGridCol: number;
	sigGridRow: number;
	outputDirForJob: string | null;
	intentRequestId: string | null;
	selectedCert: SigningCertSummary | null;
}): Promise<BatchSignBody> {
	const body: BatchSignBody = {
		cert_id_hex: opts.certIdHex.trim(),
		inputs: opts.paths,
		signature_grid: { col: opts.sigGridCol, row: opts.sigGridRow },
	};
	const pinTrim = opts.pin.trim();
	if (pinTrim) {
		body.pin = pinTrim;
	}
	try {
		const seal = await renderSignatureSealPngBase64(opts.selectedCert);
		if (seal) body.signature_seal_png_base64 = seal;
	} catch {
		/* la API usa apariencia vectorial */
	}
	if (opts.outputDirForJob) body.output_dir = opts.outputDirForJob;
	if (opts.intentRequestId) body.intent_request_id = opts.intentRequestId;
	return body;
}
