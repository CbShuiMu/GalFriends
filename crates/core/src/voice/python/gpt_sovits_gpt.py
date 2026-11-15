"""Windows 单 GPU 适配：复用官方 GPT 训练入口，避免无意义的 Gloo 进程组。"""
import argparse
from functools import partial
from pathlib import Path
import torch
import s1_train
from AR.data import data_module
from common import write_json


class AuditedDataModule(data_module.Text2SemanticDataModule):
    def setup(self, stage=None):
        super().setup(stage)
        dataset = self._train_dataset
        used = sorted(set(dataset.item_names))
        all_names = {row.split('\t')[0] for row in Path(self.train_phoneme_path).read_text('utf-8').splitlines()}
        write_json(Path(self.config['output_dir']).parent/'gpt-dataset.json',
                   dict(used=used, excluded=sorted(all_names-set(used)),
                        min_phonemes_per_second=dataset.min_ps_ratio,
                        max_phonemes_per_second=dataset.max_ps_ratio,
                        reason='official_phoneme_rate_or_length_filter'))


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--config_file', required=True)
    args = parser.parse_args()
    # 上游对任意 CUDA 设备启用 DDP；Windows 的单卡场景无需建立 Gloo。
    # 保留官方模型、数据集、优化器、回调及训练循环，多卡仍走原始策略。
    if torch.cuda.device_count() == 1:
        s1_train.DDPStrategy = lambda **_: 'auto'
        data_module.DistributedBucketSampler = partial(data_module.DistributedBucketSampler, num_replicas=1, rank=0)
    s1_train.Text2SemanticDataModule = AuditedDataModule
    s1_train.main(args)
