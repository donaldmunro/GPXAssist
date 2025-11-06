Create a python script that takes 4 parameters: path increment total sleep 

path will point to a JSON file with the following format:

[{"name":"John Doe","country":"","team":"indieVelo Founders Club","teamCode":"IVFC","power":154,"avgPower":129,"nrmPower":136,"maxPower":281,"cadence":70,"avgCadence":77,"maxCadence":90,"heartrate":126,"avgHeartrate":116,"maxHeartrate":126,"time":765,"distance":3410,"height":112,"speed":1901,"tss":9,"calories":99,"draft":0,"windSpeed":13889,"windAngle":129,"slope":11,"eventLapsTotal":1,"eventLapsDone":0,"eventDistanceTotal":49966,"eventDistanceDone":3420,"eventDistanceToNextLocation":46545,"eventNextLocation":0,"eventPosition":5}]

Note the json is not valid for two reasons:
1. There are 3 binary values 0XEF, 0XBB and 0XBF at the start of the file,
2. It is enclosed in an unnamed array i.e an opening [ and closing ]
Both the above will need to be handled if using a json library to parse and write the json.

The script should comprise a loop that:
1. reads and parses the json 
2. add the increment parameter to the "distance" field from the json file 
3. Write the modified json to a temporary file in the same directory as the original json file
4. Move (rename) the modified json file to the original json file specified by the path parameter (i.e the move should be atomic on most operating systems)
5. Pause the process for sleep time where sleep is the sleep parameter specified in seconds

The loop should terminate when the updated distance exceeds the quantity specified in the total parameter.
