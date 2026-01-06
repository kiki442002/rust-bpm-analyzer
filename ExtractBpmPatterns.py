import numpy as np
import os
from pathlib import Path


def extract_bpm_pattern(lengh: int, frame_rate: int, width: int, start_bpm: int, output_dir: str = "./patterns") -> None:
    sample = int((width+10)/0.25)
    array = np.full((sample, lengh, 32), 0, dtype=np.int64)
    jump = int(0)
    add = 0

    for i in range(sample):
        add += 0.25
        timestamp = int(60 / (start_bpm-10 + add) * frame_rate)
        jump = int(0)
        for x in range(lengh):
            timestamp_next = 0
            jump += 20
            for y in range(32):
                array[i][x][y] = timestamp_next
                timestamp_next += timestamp
            array[i][x] = array[i][x] + jump

    np.save(os.path.join(output_dir, f"{start_bpm}_bpm_pattern.npy"), array)


def extract_bpm_pattern_fine(lengh: int, frame_rate: int, width: int, start_bpm: int, output_dir: str = "./patterns") -> None:
    sample = int((width+10)/0.05)
    array = np.full((sample, lengh, 32), 0, dtype=np.int64)
    jump = int(0)
    add = 0

    for i in range(sample):
        timestamp = int(60 / (start_bpm-10 + add) * frame_rate)
        add += 0.05
        jump = int(0)
        for x in range(lengh):
            timestamp_next = 0
            jump += 20
            for y in range(32):
                array[i][x][y] = timestamp_next
                timestamp_next += timestamp
            array[i][x] = array[i][x] + jump
            
    np.save(os.path.join(output_dir, f"{start_bpm}_bpm_pattern_fine.npy"), array)


def extract(frame_rate: int, output_dir: str = "./patterns") -> None:
    os.makedirs(output_dir, exist_ok=True)
    print("PATTERN CREATOR")
    print("extracting...")
    lengh = int((frame_rate / 2) / 20)

    # 60 - 160 bpm
    extract_bpm_pattern(lengh, frame_rate, 100, 60, output_dir)
    extract_bpm_pattern_fine(lengh, frame_rate, 100, 60, output_dir)

    # 130 - 230 bpm
    extract_bpm_pattern(lengh, frame_rate, 100, 130, output_dir)
    extract_bpm_pattern_fine(lengh, frame_rate, 100, 130, output_dir)

    #200 - 300 bpm
    extract_bpm_pattern(lengh, frame_rate, 100, 210, output_dir)
    extract_bpm_pattern_fine(lengh, frame_rate, 100, 210, output_dir)

    print("\033[92m" + "COMPLETED" + "\033[0m")
    