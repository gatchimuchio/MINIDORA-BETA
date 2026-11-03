"""人工describeで、別revision・環境欠落・権限拡大の誤受入を検査する。"""
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import unittest
import secrets

from tools.CloudRun配備設定 import 固定を検査, 実行環境, 限定IAMを検査, 配備記録


IAM役割鍵 = "role"  # Google IAM policyの固定キー
実装版ラベル = "minidora-source"  # 既存配備CLIの固定ラベル


class CloudRun配備設定試験(unittest.TestCase):
    def setUp(self):
        self.設定 = json.loads((Path(__file__).resolve().parents[1] / "deploy/Google取得固定設定.json").read_text(encoding="utf-8"))
        self.SA = self.設定["MINIDORA_GOOGLE_RUNTIME_SERVICE_ACCOUNT"]
        self.審査認証 = {"MINIDORA_HTTP_BASIC_USERNAME": "judge",
                       "MINIDORA_HTTP_BASIC_PASSWORD_SHA256": hashlib.sha256(secrets.token_bytes(32)).hexdigest()}
        self.SHA = "a" * 40
        self.イメージ = "asia-northeast1-docker.pkg.dev/minidora-507702/minidora/minidora@sha256:" + "b" * 64
        self.サービス = {
            "metadata": {"name": "minidora-fixture", "generation": "7", "labels": {実装版ラベル: self.SHA},
                         "annotations": {"run.googleapis.com/minScale": "0", "run.googleapis.com/maxScale": "1"}},
            "status": {"latestReadyRevisionName": "minidora-fixture-00007", "latestCreatedRevisionName": "minidora-fixture-00007",
                       "observedGeneration": "7", "conditions": [{"type": "Ready", "status": "True"}],
                       "traffic": [{"revisionName": "minidora-fixture-00007", "percent": 100}],
                       "url": "https://minidora-fixture.run.app"},
            "spec": {"template": {"metadata": {"annotations": {"autoscaling.knative.dev/minScale": "0",
                       "autoscaling.knative.dev/maxScale": "1", "run.googleapis.com/startup-cpu-boost": "false",
                       "run.googleapis.com/cpu-throttling": "true"}},
                       "spec": {"serviceAccountName": self.SA, "containerConcurrency": 1, "timeoutSeconds": 300,
                                "containers": [{"image": self.イメージ, "command": ["python"],
                                    "args": ["-m", "minidora.製品版", "--HDS", "--ADK", "--基本チャット", "--CloudRun", "--serve"],
                                    "resources": {"limits": {"cpu": "1", "memory": "2Gi"}},
                                    "env": [{"name": 鍵, "value": 値} for 鍵, 値 in 実行環境(self.設定, "https://minidora-fixture.run.app", 審査認証=self.審査認証).items()]}]}}}}

    def 記録(self, サービス=None, IAM=None, 交換前=None, *, 審査公開=False):
        return 配備記録(self.サービス if サービス is None else サービス, {} if IAM is None else IAM, self.設定,
                        名前="minidora-fixture", リージョン="asia-northeast1", 実装SHA=self.SHA,
                        イメージ=self.イメージ, 実行SA=self.SA, 交換前=交換前,
                        審査認証=self.審査認証, 審査公開=審査公開)

    def test_固定対象とカンマを含む構造化設定を維持する(self):
        固定を検査(self.設定, "minidora-507702", "asia-northeast1", self.SA)
        env = json.loads(json.dumps(実行環境(self.設定, 審査認証=self.審査認証)))
        self.assertEqual(env["MINIDORA_GOOGLE_PUBLIC_HOSTS"], self.設定["MINIDORA_GOOGLE_PUBLIC_HOSTS"])
        self.assertTrue(self.記録()["固定環境照合"])
        for project, region in (("other-project", "asia-northeast1"), ("minidora-507702", "us-central1")):
            with self.assertRaises(ValueError): 固定を検査(self.設定, project, region, self.SA)

    def test_旧Readyと新templateと未観測世代を混同しない(self):
        for 経路, 値 in (("latestCreatedRevisionName", "minidora-fixture-00008"), ("observedGeneration", "6")):
            サービス = deepcopy(self.サービス)
            サービス["status"][経路] = 値
            with self.assertRaises(ValueError): self.記録(サービス)
        サービス = deepcopy(self.サービス)
        サービス["status"]["conditions"][0]["status"] = "False"
        with self.assertRaises(ValueError): self.記録(サービス)

    def test_交換後も実envと実資源を読み古い記録のコピーで通さない(self):
        前 = self.記録()
        新 = deepcopy(self.サービス)
        for 鍵 in ("latestCreatedRevisionName", "latestReadyRevisionName"):
            新["status"][鍵] = "minidora-fixture-00008"
        新["status"]["traffic"][0]["revisionName"] = "minidora-fixture-00008"
        self.assertNotEqual(self.記録(新, 交換前=前)["リビジョン"], 前["リビジョン"])
        新["spec"]["template"]["spec"]["containers"][0]["env"] = [
            行 for 行 in 新["spec"]["template"]["spec"]["containers"][0]["env"] if 行["name"] != "MINIDORA_GOOGLE_BUDGET_OBJECT"]
        with self.assertRaises(ValueError): self.記録(新, 交換前=前)
        with self.assertRaises(ValueError): self.記録(交換前=前)

    def test_公開IAMと実行SAの不要なservice付与を配備後にも拒否する(self):
        for member in ("allUsers", "allAuthenticatedUsers", "serviceAccount:" + self.SA):
            with self.assertRaises(ValueError):
                self.記録(IAM={"bindings": [{IAM役割鍵: "roles/run.invoker", "members": [member]}]})

    def test_限定権限と固定保存先と期限を実読取から検査する(self):
        prefix = "projects/minidora-507702/roles/"
        member = "serviceAccount:" + self.SA
        end = "request.time < timestamp('" + self.設定["MINIDORA_GOOGLE_BUDGET_END_UTC"] + "')"
        project = {"bindings": [{IAM役割鍵: prefix + "minidoraNormalSearch", "members": [member], "condition": {"expression": end}}]}
        保存権限 = {"bindings": [{IAM役割鍵: prefix + "minidoraSearchBudget", "members": [member], "condition": {"expression":
            "resource.name == 'projects/_/buckets/" + self.設定["MINIDORA_GOOGLE_BUDGET_BUCKET"] + "/objects/"
            + self.設定["MINIDORA_GOOGLE_BUDGET_OBJECT"] + "' && " + end}}]}
        検索役割 = {"name": prefix + "minidoraNormalSearch", "includedPermissions": ["discoveryengine.servingConfigs.search", "serviceusage.services.use"]}
        予算役割 = {"name": prefix + "minidoraSearchBudget", "includedPermissions": ["storage.objects.get", "storage.objects.create", "storage.objects.delete"]}
        限定IAMを検査(self.設定, project, {}, 保存権限, 検索役割, 予算役割)
        別予算 = deepcopy(保存権限)
        別予算["bindings"][0]["condition"]["expression"] = end
        with self.assertRaises(ValueError): 限定IAMを検査(self.設定, project, {}, 別予算, 検索役割, 予算役割)
        予算役割["includedPermissions"].append("storage.objects.list")
        with self.assertRaises(ValueError): 限定IAMを検査(self.設定, project, {}, 保存権限, 検索役割, 予算役割)

    def test_承認後の公開はBasic環境とallUsers_invoker一付与だけを許可する(self):
        IAM = {"bindings": [{IAM役割鍵: "roles/run.invoker", "members": ["allUsers"]}]}
        self.assertTrue(self.記録(IAM=IAM, 審査公開=True)["審査公開"])
        for policy in ({}, {"bindings": [{IAM役割鍵: "roles/owner", "members": ["allUsers"]}]},
                       {"bindings": [{IAM役割鍵: "roles/run.invoker", "members": ["allAuthenticatedUsers"]}]},
                       {"bindings": [{IAM役割鍵: "roles/run.invoker", "members": ["allUsers"], "condition": {"expression": "true"}}]}):
            with self.assertRaises(ValueError): self.記録(IAM=policy, 審査公開=True)
        サービス = deepcopy(self.サービス)
        サービス["spec"]["template"]["spec"]["containers"][0]["env"] = [
            行 for 行 in サービス["spec"]["template"]["spec"]["containers"][0]["env"]
            if 行["name"] != "MINIDORA_HTTP_BASIC_PASSWORD_SHA256"]
        with self.assertRaises(ValueError): self.記録(サービス, IAM=IAM, 審査公開=True)

    def test_IAM検査無効化と平文password追加を拒否する(self):
        サービス = deepcopy(self.サービス)
        サービス["metadata"]["annotations"]["run.googleapis.com/invoker-iam-disabled"] = "true"
        with self.assertRaises(ValueError): self.記録(サービス)
        サービス = deepcopy(self.サービス)
        サービス["spec"]["template"]["spec"]["containers"][0]["env"].append({"name": "MINIDORA_HTTP_BASIC_PASSWORD", "value": "invalid"})
        with self.assertRaises(ValueError): self.記録(サービス)

    def test_承認後は同roleの既存本人を保持したまま公開IAMを検査する(self):
        for members in (["allUsers", "user:judge-fixture@example.invalid"],
                        ["user:judge-fixture@example.invalid", "allUsers"]):
            with self.subTest(members=members):
                IAM = {"bindings": [{IAM役割鍵: "roles/run.invoker", "members": members},
                                    {IAM役割鍵: "roles/run.viewer", "members": ["user:viewer-fixture@example.invalid"]}]}
                原IAM = deepcopy(IAM)
                記録 = self.記録(IAM=IAM, 審査公開=True)
                self.assertTrue(記録["審査公開"])
                self.assertFalse(記録["CloudIAM本人認証必須"])
                self.assertEqual(記録["利用者認証"], "HTTP Basic")
                self.assertTrue(記録["認証必須"])
                self.assertEqual(IAM, 原IAM)
                with self.assertRaises(ValueError): self.記録(IAM=IAM)
                self.assertEqual(IAM, 原IAM)

    def test_本人併合で余分な公開主体とroleと条件を受け入れない(self):
        本人 = "user:judge-fixture@example.invalid"
        公開 = {IAM役割鍵: "roles/run.invoker", "members": ["allUsers", 本人]}
        不正群 = (
            {"bindings": [{IAM役割鍵: "roles/run.invoker", "members": [本人]}]},
            {"bindings": [{IAM役割鍵: "roles/run.invoker", "members": ["allUsers", "allAuthenticatedUsers", 本人]}]},
            {"bindings": [{IAM役割鍵: "roles/owner", "members": ["allUsers", 本人]}]},
            {"bindings": [{**公開, "condition": {"expression": "true"}}]},
            {"bindings": [公開, {IAM役割鍵: "roles/run.invoker", "members": ["allUsers"]}]},
            {"bindings": [公開, {IAM役割鍵: "roles/run.viewer", "members": ["allAuthenticatedUsers"]}]},
            {"bindings": [{IAM役割鍵: "roles/run.invoker", "members": ["allUsers", "serviceAccount:" + self.SA]}]},
        )
        for 番号, IAM in enumerate(不正群):
            with self.subTest(番号=番号):
                原IAM = deepcopy(IAM)
                with self.assertRaises(ValueError): self.記録(IAM=IAM, 審査公開=True)
                self.assertEqual(IAM, 原IAM)


if __name__ == "__main__":
    unittest.main()
