from pathlib import Path
import json
P=Path(__file__).parent;R=Path('/home/johnreed/Desktop/work/subtensor')
s=json.loads((R/'docs/design/evidence/ml-dsa-release-483b52ceea/validation-summary.json').read_text())
assert s['live_passed']==97 and s['shutdown']['all_closed'] and s['shutdown']['owned_processes_remaining']==0
r=s['receipt_audit'];n=r['unique_receipts'];statuses=r['status_counts'];m=s['mixed_cli_finalized_state'];assert m['all_verified']
text=(P/'report-template.md').read_text()
text=text.replace('FINAL_RECEIPT_PARAGRAPH',f"An independent pass verified **{n} unique finalized receipts** across 11 suites against canonical blocks, transaction hashes and System/wrapper events: {statuses['success']} successful outcomes, {statuses['outer_success_inner_failure']} outer-success/inner-failure outcomes and {statuses['system_failure']} System failures. Every recorded result agreed with the independently interpreted events. Expected negative cases and recorded transactions from the superseded operation attempt are retained. A separate finalized-state audit of both CLI multisig scenarios verified exact recipient balances of 8 and 9 TAO, member nonces/signing generations, released temporary deposits, and the single intended permanent proxy deposit. Receipt auditing covers persisted receipts, not a claim that every submitted extrinsic was inventoried.")
text=text.replace('FINAL_SHUTDOWN_PARAGRAPH',"All 42 owned validator processes were shut down. All 84 localnet RPC/P2P ports and the checked QR-server port 9977 were closed, with no owned localnet launchers or test processes remaining. No commits or pushes were made. The report and evidence, SDK retry fix and regressions, and CSV line-ending corrections remain local.")
assert 'FINAL_' not in text
out=R/'docs/design/ml-dsa-release-validation-483b52ceea.md';out.write_text(text);print(out)
