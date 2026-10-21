import { ApprovalDetail } from '@/components/approvals';
export default function Page({ params }: { params: { id: string } }) { return <ApprovalDetail id={params.id} />; }
