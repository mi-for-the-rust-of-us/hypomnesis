fn main() {
    println!("device_count={:?}", hypomnesis::device_count());
    println!("device_info(1)={:?}", hypomnesis::device_info(1).map(|d| d.index));
    println!("gpu_processes(1)={:?}", hypomnesis::gpu_processes(1).map(|v| v.len()));
    println!("process_gpu_info(1)={:?}", hypomnesis::process_gpu_info(1).map(|d| d.used_bytes));
}
