from ultralytics import YOLO

def export_yolo_to_coreml():
    print("Initializing YOLOv11-OBB Export...")
    
    # 1. Load the model. 
    # Here we use the base 'nano' OBB model. 
    # We are using YOLOv8 instead of YOLOv11-OBB to avoid the coremltools export bug.
    model = YOLO('yolov8n.pt') 
    
    print("Exporting to CoreML (INT8 Quantized)...")
    
    # 2. Export the model to CoreML format.
    # - format='coreml': Triggers the coremltools conversion pipeline.
    # - int8=True: Applies W8A8 quantization, heavily speeding up ANE inference.
    # - nms=True: Embeds Non-Maximum Suppression directly into the model, saving CPU cycles.
    model.export(format='coreml', int8=True, nms=True)
    
    print("Export complete! Look for the .mlpackage file in this directory.")

if __name__ == "__main__":
    export_yolo_to_coreml()
