# 开源音效方案与 Termune 接入结论

调研日期：2026-11-06。范围：普通立体声音乐，耳机空间感、低音、现场、Livehouse 和说唱/电音向的律动。以下把已核实的实现事实与工程建议分开；没有把普通混响标为测量 HRTF 或杜比全景声，也没有声称复刻酷狗的专有算法。

## 候选方案

| 方案 | 能力与边界 | 与当前 VLC 子进程架构的关系 |
| --- | --- | --- |
| VLC 自带滤镜 | 十段 EQ、动态压缩、耳机虚拟扬声器、立体声扩展、算法混响；可直接组合 | 最少额外运行依赖，但需验证加载成功及处理后的 PCM |
| libbs2b | 专为耳机立体声做交叉馈送；低阶滤波和频率相关相位；不提供房间混响或完整高频 HRTF；MIT | 适合自然、长时间耳机聆听，不是戏剧性的 3D；需获得 PCM 或添加 VLC 插件 |
| CamillaDSP | 跨 Linux/macOS/Windows 的 Rust DSP 引擎，IIR/FIR、混音、延迟、卷积、压缩与限幅；GPLv3/MPL2.0 双许可，含 ASIO 的构建仅 GPLv3 | 适合作为后续独立音频处理链路；引入进程、设备/管道与采样率协商，不宜只为几个预设让用户额外配置系统回环 |
| libmysofa | 读取 SOFA HRTF 数据，提供空间响应数据访问；主要代码 BSD-3-Clause，附带其他作者组件和数据需分别检查 | 不是即插即用音乐音效引擎；仍需卷积、声源方位、立体声映射、数据集及输出链路 |

来源：[VLC 滤镜构建清单](https://github.com/videolan/vlc/blob/3.0.x/modules/audio_filter/Makefile.am)、[bs2b 原理](https://bs2b.sourceforge.net/)、[bs2b 许可证](https://bs2b.sourceforge.net/license.html)、[CamillaDSP README](https://github.com/HEnquist/camilladsp)、[libmysofa README](https://github.com/hoene/libmysofa)、[libmysofa 许可证](https://github.com/hoene/libmysofa/blob/main/LICENSE)。

## VLC 3 模块核实

### 耳机空间处理

`headphone` 接受普通立体声，也接受单声道至多声道输入，但要求双声道输出。它按虚拟扬声器位置计算到双耳的传播时延与幅度，再将各路累加；不是测量型 HRTF 卷积。`--headphone-dim` 表示前左扬声器到听者的距离（米），不是 0..100 的强度。`--headphone-compensate` 用于补偿算法引入的时延。实际滤镜必须通过模块加载日志和左右独立测试信号确认，不能只看模块在 `--list` 中存在。

来源：[VLC headphone.c](https://github.com/videolan/vlc/blob/3.0.x/modules/audio_filter/channel_mixer/headphone.c)。

### 混响与重要风险

VLC `spatializer` 是 Jezar/Dreampoint 算法混响家族的实现，包含并联梳状与串联全通滤波器；不等于 HRTF。公开 `3.0.x` 的 `revmodel.cpp::processreplace` 中，左右干声项都用 `inputR * dry`，这是需要用本地实际 PCM 验证的声道保持风险。不能直接用这个模块替代保留原始立体声的精品现场效果。

来源：[revmodel.cpp](https://github.com/videolan/vlc/blob/3.0.x/modules/audio_filter/spatializer/revmodel.cpp)。

包装器在处理前将前两声道乘以 `0.3`；参数中的 dry 不是最终线性干声增益。内核把 dry 再乘 2，wet 乘 3；延迟线是按 44.1kHz 固定样本数设计，文件注释明确提醒高采样率需缩放。因此同名预设在不同采样率下的时间特性可能不同，不能靠单组参数宣称一致。

来源：[spatializer.cpp](https://github.com/videolan/vlc/blob/3.0.x/modules/audio_filter/spatializer/spatializer.cpp)、[tuning.h](https://github.com/videolan/vlc/blob/3.0.x/modules/audio_filter/spatializer/tuning.h)。

### EQ、扩展与动态

- `equalizer-bands` 是用空格分隔的十个 dB 值；设置自定义频段时也明确指定 `equalizer-preamp`、是否双遍与频率模式，避免继承默认/用户值。模块 preamp 的声明默认值为 +12dB，但内部还存在输入缩放，需测量实际增益。
- `stereo_widen` 要求双声道 float PCM，`stereowiden-crossfeed` 是反相减去对侧声道，会削弱左右共有的中心内容；它不是 bs2b 那种自然的交叉馈送。不要为“空间感”随意提高这个值。
- `compressor` 有 attack/release（毫秒）、threshold/knee/makeup（dB）、ratio 及 RMS/peak 配置；默认 makeup 为 +7dB。音乐预设应显式指定参数，不能把有增益补偿的压缩器叫作保证不削波的限幅器。

来源：[equalizer.c](https://github.com/videolan/vlc/blob/3.0.x/modules/audio_filter/equalizer.c)、[stereo_widen.c](https://github.com/videolan/vlc/blob/3.0.x/modules/audio_filter/stereo_widen.c)、[compressor.c](https://github.com/videolan/vlc/blob/3.0.x/modules/audio_filter/compressor.c)。

## 建议的少量预设

以下是调音目标，不是已证明的听感结论：

1. 原声：清空音效链，避免继承 VLC 用户的音效设置。
2. 醇厚低音：30Hz 高通，65Hz +8dB Q0.7、140Hz +4dB、300Hz -2.5dB；120Hz 侧声道高通，并加入低频谐波增强，让小音箱也能感到 sub，不使用极端 V 型 EQ。
3. 现场：较大场地的反射和尾音，保留清晰干声与左右定位。
4. Livehouse：35Hz 高通、120Hz +4dB、380Hz -2dB、2.8kHz +2dB、9kHz +2dB；更短的 0.42 反馈/0.55 湿声，配 2:1 压缩和 130Hz 侧声道高通，形成紧凑的小场地感。
5. 律动：70Hz +8dB 低架保证 808/sub 下潜，300Hz -3.5dB 减少浑浊，4kHz +4dB 提升人声和 lead，11kHz +5dB 增加空气感；两级 200Hz 侧声道高通用于收窄 150Hz 以下的低频立体声，中高频保留约 1.45 的宽度；2.8:1 压缩器和低频谐波增强提高密度。
6. 空间3D：7–42ms 六级交替声像早期反射、4.5kHz 低通加 0.45ms 延迟的头影交叉馈送、侧声道驱动的混响，以及 0.10Hz 的湿声场旋转；干声中心保持稳定，靠反射和旋转制造明确的外部化与移动感。

如果本地 VLC `spatializer` 确认破坏声道，应停用它或使用独立、可测试的 DSP 路径，不能只调高 dry/wet 掩盖。若引入新 DSP 库或算法，需标明其来源与许可证，不能把自写代码描述成已集成某个库。

## 本地证据与最低验证要求

本机 `vlc --version` 为 3.0.23；`vlc --list` 存在 `gain`、`compressor`、`equalizer`、`spatializer`、`stereo_widen`、`headphone`。VLC 构建清单把上述基础模块作为正常插件构建；发行版仍可删减，Windows/macOS 不能仅凭源码清单声称已验证。

验证应包括：左右单独脉冲/正弦、单声道居中信号、静音、接近满幅的宽频信号、44.1/48/96kHz，记录滤镜加载、左右能量、峰值/RMS、有限数值与混响尾音；切换时保持歌曲、暂停、位置和音量。合成信号测试只能证明处理性质，最终仍需耳机和音箱实听，并做响度尽量一致的 A/B。

### 已执行的本地 PCM 实验

使用临时生成的 1 秒 44.1kHz 双声道 WAV：左声道 440Hz 正弦（整数幅度 10000），右声道静音。VLC 3.0.23 使用 `--aout=afile --audiofile-format=float32 --no-audiofile-wav --audiofile-channels=2` 捕获实际输出，未使用麦克风、NAS 或个人音频。

| 处理 | 左 RMS | 右 RMS | 峰值 |
| --- | ---: | ---: | ---: |
| 原声 | 0.215788 | 0 | 0.305151 |
| spatializer，wet=0、dry=1 | 0 | 0 | 0 |
| spatializer，wet=.2、dry=.85 | .003630 | .001901 | .007177 |
| headphone，dim=1、compensate | .118598 | .097012 | .167833 |

这证明本机 `spatializer` 确实丢掉左路干声，不仅是从主分支源码推断。普通串联 gain、EQ、stereo_widen 都无法恢复丢失的干声；VLC 这组命令行滤镜没有原干声并联混合旋钮。因此现场/Livehouse 不应继续使用这个模块。

VLC `chorus_flanger` 是可能的短反射替代：显式关闭 sweep、保持低 wet/feedback，可保留各声道原声，适合有限的小空间早期反射。但它是单延迟反馈，不能当成扩散混响大厅；源码还有原地处理细节，应先以 PCM 测量稳态/脉冲再使用。真正的高质量场地模拟仍应采用独立且经过验证的混响或卷积处理。

来源：[chorus_flanger.c](https://github.com/videolan/vlc/blob/3.0.x/modules/audio_filter/chorus_flanger.c)。


## 本次采用的实现

最终不再用 VLC 命令行滤镜做效果：LibVLC 3 回调解码 → 有界立体声 PCM 队列 → 程序内 DSP → CPAL 默认输出设备。VLC 3 的 amem 实际只支持 S16N，通过 setup 回调明确协商采样率及两声道，然后转换为 f32 处理；不能按 VLC master 文档假定 VLC 3 提供 FL32。来源：[VLC 3 amem.c](https://github.com/videolan/vlc/blob/3.0.x/modules/audio_output/amem.c)、[LibVLC 3 回调设置](https://github.com/videolan/vlc/blob/3.0.x/lib/media_player.c)、[CPAL](https://github.com/RustAudio/cpal)。

DSP 为本项目 Rust 实现，没有集成 bs2b、CamillaDSP 或 libmysofa。采用 Jezar 的 public-domain Freeverb 并联梳状/串联全通拓扑（左右分别延迟，按设备采样率缩放，干声独立保留），RBJ 低架/峰值/高架 EQ、低频谐波增强、立体声联动峰值压缩，以及受耳机交叉馈送思想启发的头影交叉馈送；空间3D 使用 4.5kHz 低通/0.45ms 延迟的宽交叉馈送。不是 bs2b 算法的逐项复刻，也不是测量 HRTF。来源：[Freeverb 原始实现与 public-domain 声明](https://github.com/sinshu/freeverb)、[RBJ / W3C Audio EQ Cookbook](https://www.w3.org/TR/audio-eq-cookbook/)。

保留六项：原声、醇厚低音、现场、Livehouse、律动、空间3D。醇厚低音使用 65/140Hz 提升、300Hz 去泥、120Hz 侧声道高通和低频谐波增强；Livehouse 使用更短混响、presence 提升和 2:1 压缩；律动使用 70Hz 低架、300Hz 去泥、4kHz 存在感、11kHz 空气感、1.45 倍宽度、2.8:1 压缩和低频谐波增强；空间3D 使用多抽头早期反射、头影交叉馈送、侧声道混响和 0.10Hz 缓慢旋转。旧的“耳机空间”因与空间3D 重叠且侧声道提升只有约 9%，已移除；旧配置中的音效索引会自动迁移。首次切入闲置预设时先预热 50ms，再以约 150ms 渐变混合权重。淡出的预设会在权重低于 −80dB 后清空尾音并停止处理，不重新创建解码器、不 seek、不重置时间轴。峰值保护采用立体声联动增益、快速压低/平滑释放，是采样峰值保护，不宣称 true-peak 或 LUFS 母带处理。原声路径在保护未触发时保留输入数值。不是酷狗蝰蛇音效的复制。

验证包括：左右 PCM 字节解释及采样率协商、左右独立声道、满幅输入限幅、44.1/48/96kHz、预设间过渡连续性、返回原声及 seek 清尾音、现场/Livehouse 脉冲尾音差异；并实际运行 LibVLC→认证桥接→DSP→测试输出的暂停/seek/EOF/切换测试，以及 CPAL 真正设备的静音播放测试。合成信号与静音测试不代表已完成商业录音或全部平台的听感验收。

运行时仍需与程序同架构的 VLC 3；Linux 的 ALSA 是 CPAL 运行依赖，编译需要 libasound2-dev。没有增加 FFmpeg 或系统回环工具。Windows/macOS 的原生设备实测仍待完成。
