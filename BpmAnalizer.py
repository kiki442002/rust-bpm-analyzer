import numpy as np
from scipy import signal
import threading
import ExtractBpmPatterns
from threading import Thread
import traceback
import sys
from time import sleep
from pathlib import Path
import PathUtils





class BpmAnalyzer:
    def __init__(self, module, frame_rate:int=11025, start_bpm:int=60, width:int=100, coarse_steps:int=440, fine_steps:int=2200):
        self.module = module
        self.frame_rate = frame_rate
        self.start_bpm = start_bpm
        self.width = width
        self.coarse_steps = coarse_steps
        self.fine_steps = fine_steps
        self.lock = threading.Lock()
        self.stop_analyzer = threading.Event()
        
        # Get patterns directory (handles bundled vs interpreter)
        self.patterns_dir = PathUtils.get_patterns_dir()

        try:
            pattern_60 = self.patterns_dir / "60_bpm_pattern.npy"
            pattern_60_fine = self.patterns_dir / "60_bpm_pattern_fine.npy"
            pattern_130 = self.patterns_dir / "130_bpm_pattern.npy"
            pattern_130_fine = self.patterns_dir / "130_bpm_pattern_fine.npy"
            pattern_210 = self.patterns_dir / "210_bpm_pattern.npy"
            pattern_210_fine = self.patterns_dir / "210_bpm_pattern_fine.npy"
            
            self.BPM_PATTERN_60 = np.load(str(pattern_60))
            self.BPM_PATTERN_FINE_60 = np.load(str(pattern_60_fine))
            self.BPM_PATTERN_130 = np.load(str(pattern_130))
            self.BPM_PATTERN_FINE_130 = np.load(str(pattern_130_fine))
            self.BPM_PATTERN_210 = np.load(str(pattern_210))
            self.BPM_PATTERN_FINE_210 = np.load(str(pattern_210_fine))
        except FileNotFoundError:
            print(f"⏳ Generating BPM patterns in {self.patterns_dir}...")
            ExtractBpmPatterns.extract(self.frame_rate, str(self.patterns_dir))
            self.BPM_PATTERN_60 = np.load(str(self.patterns_dir / "60_bpm_pattern.npy"))
            self.BPM_PATTERN_FINE_60 = np.load(str(self.patterns_dir / "60_bpm_pattern_fine.npy"))
            self.BPM_PATTERN_130 = np.load(str(self.patterns_dir / "130_bpm_pattern.npy"))
            self.BPM_PATTERN_FINE_130 = np.load(str(self.patterns_dir / "130_bpm_pattern_fine.npy"))
            self.BPM_PATTERN_210 = np.load(str(self.patterns_dir / "210_bpm_pattern.npy"))
            self.BPM_PATTERN_FINE_210 = np.load(str(self.patterns_dir / "210_bpm_pattern_fine.npy"))
        except Exception as e:
            print("❌ Error loading BPM patterns:", e)
            traceback.print_exc()
            print("Closing application...")
            sys.exit(1)
                    

        self.bpm_pattern = self.BPM_PATTERN_60
        self.bpm_pattern_fine = self.BPM_PATTERN_FINE_60

    def search_beat_events(self, signal_array: np.ndarray, frame_rate: int) -> np.ndarray:
        step_size = frame_rate // 2
        events = []
        for step_start in range(0, len(signal_array), step_size):
            signal_array_window = signal_array[step_start : step_start + step_size]
            signal_array_window[signal_array_window < signal_array_window.max()] = 0
            signal_array_window[signal_array_window > 0] = 1
            event = np.argmax(signal_array_window) + step_start
            events.append(event)
        return np.array(events, dtype=np.int64)

    def bpm_container(self, beat_events: np.ndarray, bpm_pattern: np.ndarray, steps: int) -> list[list]:
        bpm_container = [list(np.zeros((1,), dtype=np.int64))for _ in range(beat_events.size * steps)]
        for i, beat_event in enumerate(beat_events):
            found_in_pattern = np.where(np.logical_and(bpm_pattern >= beat_event - 20, bpm_pattern <= beat_event + 20))
            for x, q in enumerate(found_in_pattern[0]):
                bpm_container[i * steps + q].append(found_in_pattern[1][x])
        return bpm_container

    def wrap_bpm_container(self, bpm_container: list, steps: int) -> list[list]:
        def flatten(input_list: list) -> list:
            return [item for sublist in input_list for item in sublist]

        bpm_container_wrapped = [list(np.zeros((1,), dtype=np.int64)) for _ in range(steps)]
        for i, w in enumerate(bpm_container_wrapped):
            w.extend(flatten(bpm_container[i::steps]))
            w.remove(0)
            bpm_container_wrapped[i] = list(filter(lambda num: num != 0, w))
        return bpm_container_wrapped

    def finalise_bpm_container(self, bpm_container_wrapped: list, steps: int) -> np.ndarray:
        bpm_container_final = np.zeros((steps, 1), dtype=np.int64)
        for i, w in enumerate(bpm_container_wrapped):
            values, counts = np.unique(w, return_counts=True)
            values = values[counts == counts.max()]
            if values[0] > 0:
                count = np.count_nonzero(w == values[0])
                bpm_container_final[i] = count
        return bpm_container_final

    def get_bpm_wrapped(self, bpm_container_final: np.ndarray) -> np.ndarray:
        return np.where(bpm_container_final == np.amax(bpm_container_final))

    def check_bpm_wrapped(self, bpm_wrapped: np.ndarray, bpm_container_final: np.ndarray) -> bool:
        count = np.count_nonzero(bpm_container_final == bpm_wrapped[0][0])
        if count > 1 or bpm_container_final[int(bpm_wrapped[0][0])] < 6:
            return 0
        else:
            return 1

    def get_bpm_pattern_fine_window(self, bpm_wrapped: np.ndarray) -> int:
        start = int(((bpm_wrapped[0][0] / 4) / 0.05) - 20)
        end = int(start + 40)
        return start, end

    def bpm_wrapped_to_float_str(self, bpm: np.ndarray, bpm_fine: np.ndarray) -> float:
        bpm_float = round(
            float((((bpm[0][0] / 4) + self.start_bpm - 10) - 1) + (bpm_fine[0][0] * 0.05)), 2
        )
        bpm_str = format(bpm_float, ".2f")
        return bpm_float, bpm_str

    def change_bpm_pattern(self, range_key: str) -> None:
        with self.lock:
            if range_key == "60–160":
                self.bpm_pattern = self.BPM_PATTERN_60
                self.bpm_pattern_fine = self.BPM_PATTERN_FINE_60
                self.start_bpm = 60
            elif range_key == "130–230":
                self.bpm_pattern = self.BPM_PATTERN_130
                self.bpm_pattern_fine = self.BPM_PATTERN_FINE_130
                self.start_bpm = 130
            elif range_key == "210–300":
                self.bpm_pattern = self.BPM_PATTERN_210
                self.bpm_pattern_fine = self.BPM_PATTERN_FINE_210
                self.start_bpm = 210


    def search_bpm(self, signal_array: np.ndarray) -> tuple:
        bpm_pattern = self.bpm_pattern
        bpm_pattern_fine = self.bpm_pattern_fine
        beat_events = self.search_beat_events(signal_array, self.frame_rate)
        for switch_pattern in [self.coarse_steps, 40]:
            bpm_container = self.bpm_container(
                beat_events, bpm_pattern, switch_pattern
            )
            bpm_container_wrapped = self.wrap_bpm_container(
                bpm_container, switch_pattern
            )
            try:
                bpm_container_final = self.finalise_bpm_container(
                    bpm_container_wrapped, switch_pattern
                )
            except ValueError:
                return 0
            bpm_wrapped = self.get_bpm_wrapped(bpm_container_final)
            if not self.check_bpm_wrapped(bpm_wrapped, bpm_container_final):
                return 0
            if switch_pattern == self.coarse_steps:
                start, end = self.get_bpm_pattern_fine_window(bpm_wrapped)
                bpm_pattern = bpm_pattern_fine[start:end]
                bpm_wrapped_full_range = bpm_wrapped
            else:
                bpm_wrapped_fine_range = bpm_wrapped
                return self.bpm_wrapped_to_float_str(
                    bpm_wrapped_full_range, bpm_wrapped_fine_range
                )

    def run_analyzer(self) -> None:
        """Main analyzer loop with error handling."""
        try:
            while not self.stop_analyzer.is_set():
                try:
                    buffer = self.module.audio_streamer.get_buffer()
                    buffer = self.bandpass_filter(buffer)
                    with self.lock:
                        if bpm_float_str := self.search_bpm(buffer):
                            self.module.bpm_storage.average_window.append(bpm_float_str[0]) 
                            bpm_average = round(
                                (
                                    sum(self.module.bpm_storage.average_window)
                                    / len(self.module.bpm_storage.average_window)
                                ),
                                2,
                            )
                            (
                                self.module.bpm_storage._float,
                                self.module.bpm_storage._str,
                            ) = bpm_average, format(bpm_average, ".2f")
                            
                            print("Detected BPM:", self.module.bpm_storage._str)
                            self.module.ui.set_bpm(self.module.bpm_storage._float)
                            self.module.ableton_link.set_bpm(self.module.bpm_storage._float)
                            
                except Exception as e:
                    print(f"❌ Error in analysis loop: {e}")
                    traceback.print_exc()
                    break
        except Exception as e:
            print(f"❌ Critical error in run_analyzer: {e}")
            traceback.print_exc()
            print("Closing application...")
            sys.exit(1)
                    

    def start_run_analyzer_thread(self, input_device_index: int) -> None:
        """Start analyzer thread with error handling."""
        try:
            self.stop_analyzer.clear()
            self.module.audio_streamer.start_stream(input_device_index=input_device_index)
            self.module.ableton_link.enable(True)
            Thread(target=self.run_analyzer, daemon=True).start()
            print("✅ BPM analyzer thread started with device index:", input_device_index)
        except Exception as e:
            print(f"❌ Error starting analyzer: {e}")
            traceback.print_exc()
            raise
        
    def stop_run_analyzer_thread(self) -> None:
        """Stop analyzer thread with error handling."""
        try:
            self.stop_analyzer.set()
            self.module.audio_streamer.stop_stream()
            self.module.ableton_link.enable(False)
            sleep(0.3)
            print("✅ BPM analyzer thread stopped.")
        except Exception as e:
            print(f"❌ Error stopping analyzer: {e}")
            traceback.print_exc()

    def butter_bandpass(self, lowcut, highcut, fs, order=10):
        """Calculate butterworth bandpass filter coefficients."""
        nyq = 0.5 * fs
        low = lowcut / nyq
        high = highcut / nyq
        b, a = signal.butter(order, [low, high], btype='band')
        return b, a

    def butter_bandpass_filter(self, data, lowcut, highcut, fs, order=10):
        """Apply butterworth bandpass filter to data."""
        b, a = self.butter_bandpass(lowcut, highcut, fs, order=order)
        y = signal.lfilter(b, a, data)
        return y

    def bandpass_filter(self, audio_signal, lowcut=60.0, highcut=3000.0) -> np.ndarray:
        """Apply bandpass filter along each axis."""
        return np.apply_along_axis(
            lambda buffer: self.butter_bandpass_filter(buffer, lowcut, highcut, self.frame_rate, order=6), 
            0, 
            audio_signal
        ).astype('int16')
